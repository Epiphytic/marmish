//! Capture and one-turn forwarding for the isolated native PostJoin proof.

use std::{
    future::Future,
    pin::Pin,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
};

use async_trait::async_trait;
use cgka_traits::{
    GroupId, MemberId, MessageId, Timestamp, TransportAccountActivation, TransportAdapter,
    TransportAdapterError, TransportDelivery, TransportEndpoint, TransportEndpointAckKind,
    TransportEndpointFailure, TransportEndpointFailureKind, TransportEndpointReceipt,
    TransportGroupSync, TransportMessage, TransportPublishFailure, TransportPublishReport,
    TransportPublishRequest, TransportPublishTarget, engine_state::PendingStateRef,
    maintenance::SignedPublicationArtifact,
};
use marmot_session::{
    KeyPackageEventDeletion, KeyPackagePublication, KeyPackagePublishError,
    KeyPackagePublishReceipt, KeyPackagePublisher, KeyPackageSlotWithdrawal,
};
use nostr::event::Event;
use transport_nostr_peeler::NostrTransportEvent;

use super::postjoin_verify::{Capture, RecoveredPostJoin};

const MAX_CANDIDATE_BYTES: usize = 64 * 1024;
const MAX_ENDPOINT_BYTES: usize = 2048;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct GateError;

#[derive(Clone, PartialEq, Eq)]
pub(super) struct Scope {
    account: MemberId,
    group: GroupId,
    transport_group: [u8; 32],
    endpoint: TransportEndpoint,
    generation: u64,
    deadline: u64,
    obligation: MessageId,
}

impl Scope {
    pub(super) fn new(
        account: MemberId,
        group: GroupId,
        transport_group: [u8; 32],
        endpoint: TransportEndpoint,
        generation: u64,
        deadline: u64,
        obligation: MessageId,
    ) -> Result<Self, GateError> {
        if account.as_slice().len() != 32
            || group.as_slice().is_empty()
            || group.as_slice().len() > 256
            || endpoint.as_str().len() > MAX_ENDPOINT_BYTES
            || !endpoint.as_str().starts_with("wss://")
            || generation == 0
            || deadline == 0
            || obligation.as_slice().len() != 32
        {
            return Err(GateError);
        }
        Ok(Self {
            account,
            group,
            transport_group,
            endpoint,
            generation,
            deadline,
            obligation,
        })
    }

    fn matches_attempt(&self, request: &TransportPublishRequest) -> bool {
        let TransportPublishTarget::Group {
            group_id,
            transport_group_id,
            endpoints,
        } = &request.target
        else {
            return false;
        };
        request.account_id == self.account
            && request.required_acks == 1
            && group_id == &self.group
            && transport_group_id.as_slice() == self.transport_group
            && endpoints.as_slice() == std::slice::from_ref(&self.endpoint)
            && request.validate_envelope_matches_target().is_ok()
    }
}

pub(super) struct Candidate {
    scope: Scope,
    request: TransportPublishRequest,
}

impl Candidate {
    pub(super) fn request(&self) -> &TransportPublishRequest {
        &self.request
    }

    fn valid(scope: &Scope, request: &TransportPublishRequest) -> bool {
        let raw = &request.message.payload;
        if !scope.matches_attempt(request) || raw.is_empty() || raw.len() > MAX_CANDIDATE_BYTES {
            return false;
        }
        let (Ok(signed), Ok(dto)) = (
            serde_json::from_slice::<Event>(raw),
            serde_json::from_slice::<NostrTransportEvent>(raw),
        ) else {
            return false;
        };
        let Ok(mut decoded) = dto.to_transport_message() else {
            return false;
        };
        // Verify the signature and the DTO's exact native routing metadata.
        // Preserve the original serialization for comparisons and delivery.
        decoded.payload.clone_from(raw);
        signed.verify().is_ok()
            && decoded.id.as_slice() == signed.id.to_bytes()
            && decoded == request.message
    }
}

#[derive(Clone, PartialEq, Eq)]
struct Operation {
    evolution: MessageId,
    obligation: MessageId,
    pending: PendingStateRef,
}

impl Operation {
    fn from_capture(capture: &Capture) -> Self {
        Self {
            evolution: capture.evolution_id().clone(),
            obligation: capture.obligation_id().clone(),
            pending: *capture.pending_ref(),
        }
    }
}

/// Only the native verifier's exact durable capture can authorize delivery.
pub(super) struct Permit {
    scope: Scope,
    operation: Operation,
    request: TransportPublishRequest,
}

impl Permit {
    pub(super) fn verify(
        scope: &Scope,
        candidate: &Candidate,
        capture: &Capture,
    ) -> Result<Self, GateError> {
        if scope != &candidate.scope || &candidate.request != capture.request() {
            return Err(GateError);
        }
        Self::from_capture(scope, capture)
    }

    pub(super) fn verify_recovery(
        scope: &Scope,
        recovered: &RecoveredPostJoin,
    ) -> Result<Self, GateError> {
        Self::from_capture(scope, recovered.capture())
    }

    /// Authorize the initial turn from exact native capture after it is
    /// durably matched to the journal. No live candidate survived the cut.
    pub(super) fn verify_reconstructed(
        scope: &Scope,
        capture: &Capture,
    ) -> Result<Self, GateError> {
        Self::from_capture(scope, capture)
    }

    fn from_capture(scope: &Scope, capture: &Capture) -> Result<Self, GateError> {
        let operation = Operation::from_capture(capture);
        let request = capture.request();
        if capture.generation() != scope.generation
            || capture.deadline() != scope.deadline
            || operation.obligation != scope.obligation
            || !Candidate::valid(scope, request)
        {
            return Err(GateError);
        }
        Ok(Self {
            scope: scope.clone(),
            operation,
            request: request.clone(),
        })
    }

    fn matches(
        &self,
        scope: &Scope,
        operation: &Operation,
        request: &TransportPublishRequest,
        wall: u64,
    ) -> bool {
        wall < scope.deadline
            && &self.scope == scope
            && &self.operation == operation
            && &self.request == request
    }
}

pub(super) enum DeliveryResult {
    Accepted,
    PossiblyExposed,
}

pub(super) type DeliverySink = Box<
    dyn FnOnce(TransportMessage) -> Pin<Box<dyn Future<Output = DeliveryResult> + Send>> + Send,
>;

#[derive(Clone, Copy, Default)]
pub(super) struct Observation {
    pub(super) commit_calls: u8,
    pub(super) deliveries: u8,
    pub(super) accepted: bool,
    pub(super) denied: u8,
    pub(super) violation: bool,
}

#[derive(Default)]
enum Mode {
    #[default]
    Closed,
    Capture,
    Armed(Box<Armed>),
    InFlight,
    Spent,
    Delivered,
}

struct Armed {
    permit: Permit,
    operation: Operation,
    sink: Option<DeliverySink>,
}

#[derive(Default)]
struct State {
    scope: Option<Scope>,
    mode: Mode,
    candidate: Option<Candidate>,
    observed: Observation,
}

#[derive(Clone)]
pub(super) struct Gate {
    wall: Arc<AtomicU64>,
    state: Arc<Mutex<State>>,
}

impl Gate {
    pub(super) fn unscoped_closed(wall: Arc<AtomicU64>) -> Self {
        Self {
            wall,
            state: Arc::new(Mutex::new(State::default())),
        }
    }

    pub(super) fn closed(scope: Scope, wall: Arc<AtomicU64>) -> Self {
        Self {
            wall,
            state: Arc::new(Mutex::new(State {
                scope: Some(scope),
                ..State::default()
            })),
        }
    }

    pub(super) fn begin_capture(&self, scope: Scope) -> Result<(), GateError> {
        let mut state = self.state.lock().map_err(|_| GateError)?;
        if state.scope.is_some()
            || !matches!(state.mode, Mode::Closed)
            || state.observed.commit_calls != 0
            || self.wall.load(Ordering::Relaxed) >= scope.deadline
        {
            return Err(GateError);
        }
        state.scope = Some(scope);
        state.mode = Mode::Capture;
        Ok(())
    }

    pub(super) fn take_candidate(&self) -> Result<Option<Candidate>, GateError> {
        let mut state = self.state.lock().map_err(|_| GateError)?;
        if !matches!(state.mode, Mode::Capture) || state.observed.violation {
            return Err(GateError);
        }
        state.mode = Mode::Spent;
        Ok(state.candidate.take())
    }

    pub(super) fn observe(&self) -> Result<Observation, GateError> {
        Ok(self.state.lock().map_err(|_| GateError)?.observed)
    }

    pub(super) fn arm(
        &self,
        permit: Permit,
        capture: &Capture,
        sink: Option<DeliverySink>,
    ) -> Result<(), GateError> {
        let mut state = self.state.lock().map_err(|_| GateError)?;
        let scope = state.scope.as_ref().ok_or(GateError)?;
        let operation = Operation::from_capture(capture);
        if !matches!(state.mode, Mode::Closed | Mode::Spent)
            || state.observed.violation
            || capture.generation() != scope.generation
            || capture.deadline() != scope.deadline
            || !permit.matches(
                scope,
                &operation,
                capture.request(),
                self.wall.load(Ordering::Relaxed),
            )
        {
            return Err(GateError);
        }
        state.mode = Mode::Armed(Box::new(Armed {
            permit,
            operation,
            sink,
        }));
        Ok(())
    }

    fn denied(&self) {
        if let Ok(mut state) = self.state.lock() {
            state.observed.denied = state.observed.denied.saturating_add(1);
        }
    }

    fn attempt(&self, request: &TransportPublishRequest) -> Option<DeliverySink> {
        let mut state = self.state.lock().ok()?;
        state.observed.commit_calls = state.observed.commit_calls.saturating_add(1);
        match std::mem::take(&mut state.mode) {
            Mode::Closed => None,
            Mode::Capture => {
                state.mode = Mode::Capture;
                let valid = state.scope.as_ref().is_some_and(|scope| {
                    self.wall.load(Ordering::Relaxed) < scope.deadline
                        && Candidate::valid(scope, request)
                });
                if state.candidate.is_none() && !state.observed.violation && valid {
                    state.candidate = Some(Candidate {
                        scope: state.scope.clone()?,
                        request: request.clone(),
                    });
                } else {
                    state.observed.violation = true;
                }
                None
            }
            Mode::Armed(armed) => {
                state.mode = Mode::Spent;
                let wall = self.wall.load(Ordering::Relaxed);
                if !state.scope.as_ref().is_some_and(|scope| {
                    armed.permit.matches(scope, &armed.operation, request, wall)
                }) {
                    state.observed.violation = true;
                    return None;
                }
                if armed.sink.is_some() {
                    state.mode = Mode::InFlight;
                    state.observed.deliveries = state.observed.deliveries.saturating_add(1);
                }
                armed.sink
            }
            mode @ (Mode::InFlight | Mode::Spent | Mode::Delivered) => {
                state.mode = mode;
                state.observed.violation = true;
                None
            }
        }
    }
    fn finish_delivery(&self, accepted: bool) {
        if let Ok(mut state) = self.state.lock() {
            state.observed.accepted = accepted;
            state.mode = if accepted {
                Mode::Delivered
            } else {
                Mode::Spent
            };
        }
    }
}

fn refused(
    request: &TransportPublishRequest,
    kind: TransportEndpointFailureKind,
) -> TransportAdapterError {
    let [endpoint] = request.target.endpoints() else {
        return TransportAdapterError::Closed;
    };
    if endpoint.as_str().len() > MAX_ENDPOINT_BYTES {
        return TransportAdapterError::Closed;
    }
    let pre_send = kind == TransportEndpointFailureKind::RetryableUnavailable;
    let reason = if pre_send {
        "peer_postjoin_gate_refused"
    } else {
        "peer_postjoin_delivery_unconfirmed"
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

#[async_trait]
impl TransportAdapter for Gate {
    async fn activate_account(
        &self,
        _: TransportAccountActivation,
    ) -> Result<(), TransportAdapterError> {
        self.denied();
        Err(TransportAdapterError::Closed)
    }

    async fn sync_account_groups(
        &self,
        _: TransportGroupSync,
    ) -> Result<(), TransportAdapterError> {
        self.denied();
        Err(TransportAdapterError::Closed)
    }

    async fn deactivate_account(&self, _: &MemberId) -> Result<(), TransportAdapterError> {
        self.denied();
        Err(TransportAdapterError::Closed)
    }

    async fn publish(
        &self,
        _: TransportPublishRequest,
    ) -> Result<TransportPublishReport, TransportAdapterError> {
        self.denied();
        Err(TransportAdapterError::Closed)
    }

    async fn receive(&self) -> Result<Option<TransportDelivery>, TransportAdapterError> {
        self.denied();
        Err(TransportAdapterError::Closed)
    }

    async fn publish_commit(
        &self,
        request: TransportPublishRequest,
    ) -> Result<TransportPublishReport, TransportAdapterError> {
        let Some(send) = self.attempt(&request) else {
            return Err(refused(
                &request,
                TransportEndpointFailureKind::RetryableUnavailable,
            ));
        };
        let [endpoint] = request.target.endpoints() else {
            return Err(TransportAdapterError::Closed);
        };
        let failure = refused(&request, TransportEndpointFailureKind::PossiblyExposed);
        let message_id = request.message.id.clone();
        let endpoint = endpoint.clone();
        let delivered = send(request.message).await;
        self.finish_delivery(matches!(delivered, DeliveryResult::Accepted));
        match delivered {
            DeliveryResult::Accepted => Ok(TransportPublishReport {
                message_id,
                accepted: vec![TransportEndpointReceipt {
                    endpoint,
                    accepted_at: Some(Timestamp(self.wall.load(Ordering::Relaxed))),
                    ack_kind: Some(TransportEndpointAckKind::Affirmative),
                }],
                failed: Vec::new(),
                required_acks: 1,
            }),
            DeliveryResult::PossiblyExposed => Err(failure),
        }
    }
}

fn package_refused<T>(gate: &Gate) -> Result<T, KeyPackagePublishError> {
    gate.denied();
    Err(KeyPackagePublishError::unexposed(
        "peer_postjoin_package_refused",
    ))
}

#[async_trait]
impl KeyPackagePublisher for Gate {
    fn legacy_slot_id(&self, _: &MemberId) -> Result<Option<String>, KeyPackagePublishError> {
        package_refused(self)
    }

    async fn prepare_key_package(
        &self,
        _: KeyPackagePublication,
    ) -> Result<SignedPublicationArtifact, KeyPackagePublishError> {
        package_refused(self)
    }

    async fn publish_prepared_key_package(
        &self,
        _: &KeyPackagePublication,
        _: &SignedPublicationArtifact,
    ) -> Result<KeyPackagePublishReceipt, KeyPackagePublishError> {
        package_refused(self)
    }

    async fn withdraw_key_package_slot(
        &self,
        _: &KeyPackageSlotWithdrawal,
    ) -> Result<KeyPackagePublishReceipt, KeyPackagePublishError> {
        package_refused(self)
    }

    async fn delete_key_package_event(
        &self,
        _: &KeyPackageEventDeletion,
    ) -> Result<KeyPackagePublishReceipt, KeyPackagePublishError> {
        package_refused(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nostr::{
        event::{EventBuilder, FinalizeEvent, Kind, Tag},
        key::Keys,
    };

    const NOW: u64 = 100;
    const TRANSPORT_GROUP: [u8; 32] = [3; 32];

    fn scope() -> Scope {
        Scope::new(
            MemberId::new([1; 32]),
            GroupId::new([2; 32]),
            TRANSPORT_GROUP,
            TransportEndpoint::from("wss://localhost:28111"),
            1,
            NOW + 60,
            MessageId::new([4; 32]),
        )
        .unwrap()
    }

    fn request(scope: &Scope) -> TransportPublishRequest {
        let signed = EventBuilder::new(Kind::Custom(445), "test")
            .tag(Tag::parse(["h", &hex::encode(TRANSPORT_GROUP)]).unwrap())
            .custom_created_at(nostr::types::Timestamp::from_secs(NOW))
            .finalize(&Keys::generate())
            .unwrap();
        let bytes = serde_json::to_vec(&signed).unwrap();
        let dto: NostrTransportEvent = serde_json::from_slice(&bytes).unwrap();
        let mut message = dto.to_transport_message().unwrap();
        message.payload = bytes;
        TransportPublishRequest {
            account_id: scope.account.clone(),
            message,
            target: TransportPublishTarget::Group {
                group_id: scope.group.clone(),
                transport_group_id: scope.transport_group.to_vec(),
                endpoints: vec![scope.endpoint.clone()],
            },
            required_acks: 1,
        }
    }

    fn operation(scope: &Scope) -> Operation {
        Operation {
            evolution: MessageId::new([5; 32]),
            obligation: scope.obligation.clone(),
            pending: PendingStateRef::new(6),
        }
    }

    #[test]
    fn exact_permit_rejects_every_scope_or_operation_move() {
        let scope = scope();
        let request = request(&scope);
        let operation = operation(&scope);
        let permit = Permit {
            scope: scope.clone(),
            operation: operation.clone(),
            request: request.clone(),
        };
        assert!(permit.matches(&scope, &operation, &request, NOW));
        for change in [
            "endpoint",
            "generation",
            "operation",
            "obligation",
            "pending",
            "bytes",
            "expiry",
        ] {
            let mut scope = scope.clone();
            let mut operation = operation.clone();
            let mut request = request.clone();
            let mut wall = NOW;
            match change {
                "endpoint" => scope.endpoint = TransportEndpoint::from("wss://localhost:28112"),
                "generation" => scope.generation += 1,
                "operation" => operation.evolution = MessageId::new([7; 32]),
                "obligation" => operation.obligation = MessageId::new([8; 32]),
                "pending" => operation.pending = PendingStateRef::new(9),
                "bytes" => request.message.payload.push(b' '),
                "expiry" => wall = scope.deadline,
                _ => unreachable!(),
            }
            assert!(
                !permit.matches(&scope, &operation, &request, wall),
                "{change}"
            );
        }
    }

    #[tokio::test]
    async fn capture_is_bounded_and_refusals_are_typed() {
        let scope = scope();
        let request = request(&scope);
        let wall = Arc::new(AtomicU64::new(NOW));
        let gate = Gate::unscoped_closed(wall.clone());
        gate.begin_capture(scope.clone()).unwrap();
        let error = gate.publish_commit(request.clone()).await.unwrap_err();
        let [failure] = error.publish_endpoint_failures() else {
            panic!("one pre-send failure");
        };
        assert_eq!(
            failure.kind,
            TransportEndpointFailureKind::RetryableUnavailable
        );
        assert!(failure.pre_send);
        assert_eq!(gate.take_candidate().unwrap().unwrap().request(), &request);
        assert!(gate.publish_commit(request.clone()).await.is_err());
        assert!(gate.observe().unwrap().violation);
        assert_eq!(gate.observe().unwrap().deliveries, 0);

        for changed in [false, true] {
            let duplicate = Gate::unscoped_closed(wall.clone());
            duplicate.begin_capture(scope.clone()).unwrap();
            let mut second = request.clone();
            if changed {
                second.message.payload.push(b' ');
            }
            assert!(duplicate.publish_commit(request.clone()).await.is_err());
            assert!(duplicate.publish_commit(second).await.is_err());
            assert!(duplicate.take_candidate().is_err());
            assert_eq!(duplicate.observe().unwrap().deliveries, 0);
        }
        let oversized = Gate::unscoped_closed(wall.clone());
        oversized.begin_capture(scope.clone()).unwrap();
        let mut large = request.clone();
        large.message.payload.resize(MAX_CANDIDATE_BYTES + 1, b' ');
        assert!(oversized.publish_commit(large).await.is_err());
        assert!(oversized.take_candidate().is_err());
        assert_eq!(oversized.observe().unwrap().deliveries, 0);

        let closed = Gate::closed(scope.clone(), wall.clone());
        assert!(closed.publish(request.clone()).await.is_err());
        assert_eq!(closed.observe().unwrap().denied, 1);
        assert_eq!(
            closed
                .read_back(&scope.account, &request.message, &[scope.endpoint])
                .await,
            [cgka_traits::TransportReadBack::NoAnswer]
        );
    }

    #[tokio::test]
    async fn unconfirmed_delivery_is_possibly_exposed() {
        let scope = scope();
        let request = request(&scope);
        let operation = operation(&scope);
        let gate = Gate::closed(scope.clone(), Arc::new(AtomicU64::new(NOW)));
        let sent = request.message.clone();
        gate.state.lock().unwrap().mode = Mode::Armed(Box::new(Armed {
            permit: Permit {
                scope,
                operation: operation.clone(),
                request: request.clone(),
            },
            operation,
            sink: Some(Box::new(move |message| {
                Box::pin(async move {
                    assert_eq!(message, sent);
                    DeliveryResult::PossiblyExposed
                })
            })),
        }));
        let error = gate.publish_commit(request).await.unwrap_err();
        let [failure] = error.publish_endpoint_failures() else {
            panic!("one possibly exposed failure");
        };
        assert_eq!(failure.kind, TransportEndpointFailureKind::PossiblyExposed);
        assert!(!failure.pre_send);
        assert_eq!(gate.observe().unwrap().deliveries, 1);
        assert!(!gate.observe().unwrap().accepted);
    }
}
