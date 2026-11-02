//! Explicit, bounded live acceptance driver; never invoked by default tests.
use crate::{
    Error, Result,
    daemon::{self, Config},
    identity::{BackendChoice, ProfileLease},
    mdk_adapter::MdkOwner,
    transport::{ArtiDirs, RelayConnector, RelayPolicy, RouteMode, TransportLimits},
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::os::unix::fs::PermissionsExt;
use std::{
    path::{Path, PathBuf},
    time::Duration,
};
use tokio::time::Instant;

#[derive(Serialize)]
struct Sample {
    stage: &'static str,
    milliseconds: u128,
    bytes: usize,
    sha256: String,
    bytes_per_second: Option<u64>,
}

fn policy(relays: &[String], mode: RouteMode) -> Result<RelayPolicy> {
    RelayPolicy::parse(relays, mode, TransportLimits::default())
}

fn config(
    root: &Path,
    name: &str,
    relays: &[String],
    mode: RouteMode,
    peer: String,
) -> Result<Config> {
    Ok(Config {
        state_dir: root.to_owned(),
        identity: name.to_owned(),
        backend: BackendChoice::File,
        helper: std::env::current_exe()?,
        relay_policy: policy(relays, mode)?,
        allowed_peers: vec![peer],
        retention: Duration::from_secs(86400),
    })
}

#[derive(Default)]
struct Pair {
    alice: Option<ProfileLease>,
    bob: Option<ProfileLease>,
    a: Option<MdkOwner>,
    b: Option<MdkOwner>,
    cleanup_failed: bool,
}
impl Pair {
    fn release(&mut self) -> bool {
        // Drop cancels the transport before touching either runtime directory.
        drop(self.a.take());
        drop(self.b.take());
        let a = self
            .alice
            .as_ref()
            .is_none_or(|p| p.cleanup_runtime().is_ok());
        let b = self
            .bob
            .as_ref()
            .is_none_or(|p| p.cleanup_runtime().is_ok());
        drop(self.alice.take());
        drop(self.bob.take());
        self.cleanup_failed |= !(a && b);
        !self.cleanup_failed
    }
    async fn cleanup(&mut self, deadline: Instant) -> bool {
        let (a, b) = (&mut self.a, &mut self.b);
        async fn shutdown(owner: &mut Option<MdkOwner>, deadline: Instant) -> bool {
            match owner {
                Some(owner) => matches!(
                    tokio::time::timeout_at(deadline, owner.shutdown(deadline)).await,
                    Ok(Ok(()))
                ),
                None => true,
            }
        }
        let (a, b) = tokio::join!(shutdown(a, deadline), shutdown(b, deadline));
        let paths = self.release();
        self.cleanup_failed |= !(a && b && paths);
        !self.cleanup_failed
    }
}
impl Drop for Pair {
    fn drop(&mut self) {
        self.release();
    }
}

async fn create_pair(
    pair: &mut Pair,
    root: PathBuf,
    relays: &[String],
    mode: RouteMode,
) -> Result<()> {
    let (prepared, keys) = tokio::task::spawn_blocking(move || {
        let mut prepared = Pair::default();
        let keys =
            (|| {
                let helper = std::env::current_exe()?;
                prepared.alice = Some(ProfileLease::acquire(&root, "alice")?);
                prepared.bob = Some(ProfileLease::acquire(&root, "bob")?);
                let deadline = std::time::Instant::now() + Duration::from_secs(30);
                let alice = prepared.alice.as_mut().unwrap().prepare(
                    BackendChoice::File,
                    &helper,
                    deadline,
                )?;
                let bob = prepared.bob.as_mut().unwrap().prepare(
                    BackendChoice::File,
                    &helper,
                    deadline,
                )?;
                Ok::<_, Error>((alice, bob))
            })();
        (prepared, keys)
    })
    .await
    .map_err(|_| Error::Identity("initialization_worker"))?;
    *pair = prepared;
    let (alice_keys, bob_keys) = keys?;
    let connector = |profile: &ProfileLease| {
        RelayConnector::new(
            policy(relays, mode)?,
            ArtiDirs {
                state: profile.arti_state().to_owned(),
                cache: profile.arti_cache().to_owned(),
            },
        )
    };
    let alice = pair.alice.as_mut().unwrap();
    pair.a = Some(MdkOwner::open(
        alice,
        &alice_keys,
        connector(alice)?,
        vec![bob_keys.public_key().to_hex()],
    )?);
    alice.mark_ready(pair.a.as_ref().unwrap().opened_binding())?;
    let bob = pair.bob.as_mut().unwrap();
    pair.b = Some(MdkOwner::open(
        bob,
        &bob_keys,
        connector(bob)?,
        vec![alice_keys.public_key().to_hex()],
    )?);
    bob.mark_ready(pair.b.as_ref().unwrap().opened_binding())?;
    Ok(())
}

#[derive(Serialize)]
struct Recoverable {
    role: &'static str,
    label: &'static str,
}
#[derive(Serialize)]
struct Diagnostic {
    operation: &'static str,
    last_recoverable: Option<Recoverable>,
    terminal_label: Option<&'static str>,
}
impl Default for Diagnostic {
    fn default() -> Self {
        Self {
            operation: "none",
            last_recoverable: None,
            terminal_label: None,
        }
    }
}
fn terminal_transport(label: &str) -> bool {
    matches!(
        label,
        "tls_verification"
            | "tls_hostname"
            | "proxy_forbidden"
            | "arti_configuration"
            | "relay_not_configured"
            | "shutdown"
            | "delivery_closed"
            | "notification_forwarder_stopped"
    )
}
fn diagnostic_label(label: &'static str) -> &'static str {
    if terminal_transport(label)
        || matches!(
            label,
            "timeout"
                | "transport_recovery_pending"
                | "subscription_sync"
                | "journal_backpressure"
                | "relay_reconnect"
                | "recovery_subscription"
                | "notification_loss"
                | "group_state_unavailable"
        )
    {
        label
    } else {
        "other"
    }
}
impl Diagnostic {
    fn begin(&mut self, operation: &'static str) {
        self.operation = operation;
        self.last_recoverable = None;
        self.terminal_label = None;
    }
    // Any transport error ending the run is terminal here, including errors
    // from one-shot stages whose labels would be retryable during a tick.
    fn failure(&mut self, error: &Error) {
        self.terminal_label = match error {
            Error::Transport(label) => Some(diagnostic_label(label)),
            _ => None,
        };
    }
}

trait ExchangeOwner {
    async fn tick(&mut self, deadline: Instant) -> Result<()>;
    async fn send(&mut self, session: &str, id: &str, data: &[u8], deadline: Instant)
    -> Result<()>;
    fn joined(&self, session: &str) -> Result<bool>;
    fn received_bytes(&self, session: &str, id: &str) -> Result<Option<Vec<u8>>>;
    fn receipt_received(&self, session: &str, id: &str) -> Result<bool>;
}
impl ExchangeOwner for MdkOwner {
    async fn tick(&mut self, deadline: Instant) -> Result<()> {
        MdkOwner::tick(self, deadline).await
    }
    async fn send(
        &mut self,
        session: &str,
        id: &str,
        data: &[u8],
        deadline: Instant,
    ) -> Result<()> {
        MdkOwner::send(self, session, id, data, deadline)
            .await
            .map(|_| ())
    }
    fn joined(&self, session: &str) -> Result<bool> {
        Ok(self
            .conversations()?
            .iter()
            .any(|c| c.session == session && !c.revoked))
    }
    fn received_bytes(&self, session: &str, id: &str) -> Result<Option<Vec<u8>>> {
        Ok(self
            .received()?
            .into_iter()
            .find(|m| m.session == session && m.application_id == id && !m.invalidated)
            .map(|m| m.data))
    }
    fn receipt_received(&self, session: &str, id: &str) -> Result<bool> {
        Ok(self.sent_status(session, id)?.state == "received")
    }
}
async fn owner_turn(
    owner: &mut impl ExchangeOwner,
    role: &'static str,
    deadline: Instant,
    diagnostic: &mut Diagnostic,
) -> Result<()> {
    diagnostic.operation = if role == "receiver" {
        "receiver_tick"
    } else {
        "sender_tick"
    };
    if Instant::now() >= deadline {
        return Err(Error::Timeout);
    }
    let outcome = owner
        .tick(deadline.min(Instant::now() + Duration::from_secs(15)))
        .await;
    match outcome {
        Ok(()) => {}
        Err(Error::Transport(label)) if !terminal_transport(label) => {
            diagnostic.last_recoverable = Some(Recoverable {
                role,
                label: diagnostic_label(label),
            });
        }
        Err(Error::Timeout) => {
            diagnostic.last_recoverable = Some(Recoverable {
                role,
                label: "timeout",
            });
        }
        Err(error) => return Err(error),
    }
    if Instant::now() >= deadline {
        return Err(Error::Timeout);
    }
    Ok(())
}
enum Awaiting<'a> {
    Join {
        session: &'a str,
    },
    Message {
        session: &'a str,
        id: &'a str,
        bytes: &'a [u8],
    },
}
async fn drive_until(
    sender: &mut impl ExchangeOwner,
    receiver: &mut impl ExchangeOwner,
    awaiting: Awaiting<'_>,
    deadline: Instant,
    diagnostic: &mut Diagnostic,
) -> Result<()> {
    diagnostic.begin(match &awaiting {
        Awaiting::Join { .. } => "join",
        Awaiting::Message { .. } => "receiver_tick",
    });
    loop {
        owner_turn(receiver, "receiver", deadline, diagnostic).await?;
        owner_turn(sender, "sender", deadline, diagnostic).await?;
        let ready = match &awaiting {
            Awaiting::Join { session } => {
                diagnostic.operation = "join";
                receiver.joined(session)?
            }
            Awaiting::Message { session, id, bytes } => {
                diagnostic.operation = "received";
                match receiver.received_bytes(session, id)? {
                    Some(received) => {
                        if received != *bytes {
                            return Err(Error::Mdk("byte_preservation"));
                        }
                        diagnostic.operation = "sent_status";
                        sender.receipt_received(session, id)?
                    }
                    None => false,
                }
            }
        };
        if ready {
            return Ok(());
        }
        tokio::time::sleep_until(deadline.min(Instant::now() + Duration::from_millis(100))).await;
    }
}

struct ExchangeMessage<'a> {
    session: &'a str,
    id: &'a str,
    bytes: &'a [u8],
}
async fn exchange(
    sender: &mut impl ExchangeOwner,
    receiver: &mut impl ExchangeOwner,
    message: ExchangeMessage<'_>,
    stage: &'static str,
    overall: Instant,
    diagnostic: &mut Diagnostic,
) -> Result<Sample> {
    let ExchangeMessage { session, id, bytes } = message;
    let start = Instant::now();
    let deadline = overall.min(start + Duration::from_secs(120));
    diagnostic.begin("initial_send");
    sender.send(session, id, bytes, deadline).await?;
    drive_until(
        sender,
        receiver,
        Awaiting::Message { session, id, bytes },
        deadline,
        diagnostic,
    )
    .await?;
    diagnostic.operation = "duplicate_send";
    sender.send(session, id, bytes, deadline).await?;
    let mut changed = bytes.to_vec();
    changed.push(1);
    diagnostic.operation = "conflict_send";
    match sender.send(session, id, &changed, deadline).await {
        Err(Error::Invalid("application_id_conflict")) => {}
        Err(error) => return Err(error),
        Ok(()) => return Err(Error::Mdk("conflicting_application_id_accepted")),
    }
    Ok(Sample {
        stage,
        milliseconds: start.elapsed().as_millis(),
        bytes: bytes.len(),
        sha256: hex::encode(Sha256::digest(bytes)),
        bytes_per_second: Some(
            ((bytes.len() as u128) * 1_000_000_000 / start.elapsed().as_nanos().max(1)) as u64,
        ),
    })
}

struct CheckProgress<'a> {
    samples: &'a mut Vec<Sample>,
    stage: &'a mut &'static str,
    diagnostic: &'a mut Diagnostic,
}
async fn execute(
    pair: &mut Pair,
    root: &Path,
    relays: &[String],
    mode: RouteMode,
    overall: Instant,
    progress: CheckProgress<'_>,
) -> Result<()> {
    let CheckProgress {
        samples,
        stage,
        diagnostic,
    } = progress;
    crate::diagnostics::stage(stage, "identity_prepare");
    create_pair(pair, root.to_owned(), relays, mode).await?;
    let (a, b) = (pair.a.as_mut().unwrap(), pair.b.as_mut().unwrap());
    crate::diagnostics::stage(stage, "activation_cold");
    let began = Instant::now();
    diagnostic.begin("activation");
    a.activate(overall.min(began + Duration::from_secs(150)))
        .await?;
    diagnostic.begin("activation");
    b.activate(overall.min(Instant::now() + Duration::from_secs(150)))
        .await?;
    samples.push(Sample {
        stage: "activation_cold",
        milliseconds: began.elapsed().as_millis(),
        bytes: 0,
        sha256: String::new(),
        bytes_per_second: None,
    });
    let a_key = a.status()?.identity;
    let b_key = b.status()?.identity;
    crate::diagnostics::stage(stage, "conversation_open");
    diagnostic.begin("open_conversation");
    let conversation = a
        .open_conversation(
            &b_key,
            overall.min(Instant::now() + Duration::from_secs(30)),
        )
        .await?;
    crate::diagnostics::stage(stage, "conversation_join");
    let join_deadline = overall.min(Instant::now() + Duration::from_secs(90));
    drive_until(
        a,
        b,
        Awaiting::Join {
            session: &conversation.session,
        },
        join_deadline,
        diagnostic,
    )
    .await?;
    let edge=b"\0\x01\x7f\xff quotes:'\" backslash:\\ dollar:$ newline:\n cr:\r tab:\t utf8:\xf0\x9f\xa6\xab";
    crate::diagnostics::stage(stage, "alice_to_bob");
    samples.push(
        exchange(
            a,
            b,
            ExchangeMessage {
                session: &conversation.session,
                id: &crate::foundation::fresh_id(
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map_err(|_| Error::Invalid("clock"))?
                        .as_secs(),
                )?,
                bytes: edge,
            },
            stage,
            overall,
            diagnostic,
        )
        .await?,
    );
    crate::diagnostics::stage(stage, "bob_to_alice");
    samples.push(
        exchange(
            b,
            a,
            ExchangeMessage {
                session: &conversation.session,
                id: &crate::foundation::fresh_id(
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map_err(|_| Error::Invalid("clock"))?
                        .as_secs(),
                )?,
                bytes: edge,
            },
            stage,
            overall,
            diagnostic,
        )
        .await?,
    );
    crate::diagnostics::stage(stage, "restart_shutdown");
    if !pair
        .cleanup(overall.min(Instant::now() + Duration::from_secs(15)))
        .await
    {
        return Err(Error::Mdk("restart_cleanup"));
    }
    crate::diagnostics::stage(stage, "restart_open");
    diagnostic.begin("activation");
    let began = Instant::now();
    let (profile, owner) = daemon::open(config(root, "alice", relays, mode, b_key)?).await?;
    pair.alice = Some(profile);
    pair.a = Some(owner);
    let (profile, owner) = daemon::open(config(root, "bob", relays, mode, a_key)?).await?;
    pair.bob = Some(profile);
    pair.b = Some(owner);
    let (a, b) = (pair.a.as_mut().unwrap(), pair.b.as_mut().unwrap());
    crate::diagnostics::stage(stage, "restart_activation_warm");
    diagnostic.begin("activation");
    a.activate(overall.min(Instant::now() + Duration::from_secs(150)))
        .await?;
    diagnostic.begin("activation");
    b.activate(overall.min(Instant::now() + Duration::from_secs(150)))
        .await?;
    samples.push(Sample {
        stage: "restart_activation_warm",
        milliseconds: began.elapsed().as_millis(),
        bytes: 0,
        sha256: String::new(),
        bytes_per_second: None,
    });
    crate::diagnostics::stage(stage, "restart_alice_to_bob");
    samples.push(
        exchange(
            a,
            b,
            ExchangeMessage {
                session: &conversation.session,
                id: &crate::foundation::fresh_id(
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map_err(|_| Error::Invalid("clock"))?
                        .as_secs(),
                )?,
                bytes: edge,
            },
            stage,
            overall,
            diagnostic,
        )
        .await?,
    );
    crate::diagnostics::stage(stage, "restart_bob_to_alice");
    samples.push(
        exchange(
            b,
            a,
            ExchangeMessage {
                session: &conversation.session,
                id: &crate::foundation::fresh_id(
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map_err(|_| Error::Invalid("clock"))?
                        .as_secs(),
                )?,
                bytes: edge,
            },
            stage,
            overall,
            diagnostic,
        )
        .await?,
    );
    diagnostic.operation = "none";
    crate::diagnostics::stage(stage, "completed");
    Ok(())
}
fn category(
    result: &std::result::Result<Result<()>, tokio::time::error::Elapsed>,
) -> Option<&'static str> {
    match result {
        Ok(Ok(())) => None,
        Ok(Err(Error::Timeout)) | Err(_) => Some("deadline"),
        Ok(Err(Error::Identity(_))) => Some("identity"),
        Ok(Err(Error::Storage(_))) => Some("storage"),
        Ok(Err(Error::Transport(_))) => Some("transport"),
        Ok(Err(Error::Mdk(_))) => Some("marmot"),
        Ok(Err(_)) => Some("local_setup"),
    }
}
fn recover_runtime_paths(root: &Path) -> bool {
    let mut clean = true;
    for name in ["alice", "bob"] {
        match std::fs::symlink_metadata(root.join(name)) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(_) => {
                clean = false;
                continue;
            }
            Ok(_) => (),
        }
        match ProfileLease::acquire(root, name) {
            Ok(profile) => {
                if profile.cleanup_runtime().is_err() {
                    clean = false;
                }
            }
            Err(_) => clean = false,
        }
    }
    clean
}
pub async fn run(relays: Vec<String>, mode: RouteMode) -> Result<serde_json::Value> {
    policy(&relays, mode)?;
    let root = tempfile::Builder::new()
        .prefix("marmish-v2-foundation-")
        .permissions(std::fs::Permissions::from_mode(0o700))
        .tempdir()?;
    let start = Instant::now();
    let overall = start + Duration::from_secs(600);
    let execution = overall - Duration::from_secs(30);
    let mut pair = Pair::default();
    let mut samples = Vec::new();
    let mut stage = "identity_prepare";
    let mut diagnostic = Diagnostic::default();
    let result = tokio::time::timeout_at(
        execution,
        execute(
            &mut pair,
            root.path(),
            &relays,
            mode,
            execution,
            CheckProgress {
                samples: &mut samples,
                stage: &mut stage,
                diagnostic: &mut diagnostic,
            },
        ),
    )
    .await;
    if let Ok(Err(error)) = &result {
        diagnostic.failure(error);
    }
    let mut category = category(&result);
    let owner_cleanup = pair.cleanup(overall).await;
    drop(pair);
    let path_cleanup = recover_runtime_paths(root.path());
    let runtime_cleanup = owner_cleanup && path_cleanup;
    let state_cleanup = root.close().is_ok();
    let cleanup_ok = runtime_cleanup && state_cleanup;
    if category.is_none() && !cleanup_ok {
        category = Some("cleanup");
        stage = "cleanup";
    }
    let route = match mode {
        RouteMode::Auto => "auto",
        RouteMode::Always => "always",
        RouteMode::Off => "off",
    };
    Ok(
        serde_json::json!({"diagnostic":diagnostic,"schema":1,"protocol":crate::foundation::PROTOCOL,"phase":"v0.2-phase1","stage":stage,"route":route,"ok":category.is_none()&&cleanup_ok,"error":category,"category":category,"target":std::env::consts::ARCH,"platform":std::env::consts::OS,"relay_count":relays.len(),"elapsed_ms":start.elapsed().as_millis(),"samples":samples,"cleanup_ok":cleanup_ok,"runtime_cleanup_ok":runtime_cleanup,"state_cleanup_ok":state_cleanup}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[derive(Clone, Copy)]
    enum Turn {
        Good,
        Transport(&'static str),
        Timeout,
        Fatal,
    }
    struct ScriptedOwner {
        script: std::collections::VecDeque<Turn>,
        default: Turn,
        ticks: Vec<(Instant, Instant)>,
        sends: Vec<Vec<u8>>,
        send_errors: std::collections::VecDeque<Option<Error>>,
        ready_after: usize,
        bytes: Vec<u8>,
    }
    impl ScriptedOwner {
        fn new(script: &[Turn]) -> Self {
            Self {
                script: script.iter().copied().collect(),
                default: Turn::Good,
                ticks: Vec::new(),
                sends: Vec::new(),
                send_errors: Default::default(),
                ready_after: 1,
                bytes: b"payload".to_vec(),
            }
        }
    }
    impl ExchangeOwner for ScriptedOwner {
        async fn tick(&mut self, deadline: Instant) -> Result<()> {
            self.ticks.push((Instant::now(), deadline));
            match self.script.pop_front().unwrap_or(self.default) {
                Turn::Good => Ok(()),
                Turn::Transport(label) => Err(Error::Transport(label)),
                Turn::Fatal => Err(Error::Storage("scripted")),
                Turn::Timeout => {
                    tokio::time::sleep_until(deadline).await;
                    Err(Error::Timeout)
                }
            }
        }
        async fn send(&mut self, _: &str, _: &str, data: &[u8], _: Instant) -> Result<()> {
            self.sends.push(data.to_vec());
            if let Some(error) = self.send_errors.pop_front() {
                return error.map_or(Ok(()), Err);
            }
            if data != self.bytes {
                Err(Error::Invalid("application_id_conflict"))
            } else {
                Ok(())
            }
        }
        fn joined(&self, _: &str) -> Result<bool> {
            Ok(self.ticks.len() >= self.ready_after)
        }
        fn received_bytes(&self, _: &str, _: &str) -> Result<Option<Vec<u8>>> {
            Ok((self.ticks.len() >= self.ready_after).then(|| self.bytes.clone()))
        }
        fn receipt_received(&self, _: &str, _: &str) -> Result<bool> {
            Ok(self.ticks.len() >= self.ready_after)
        }
    }
    async fn scripted_exchange(
        sender: &mut ScriptedOwner,
        receiver: &mut ScriptedOwner,
        d: &mut Diagnostic,
    ) -> Result<Sample> {
        exchange(
            sender,
            receiver,
            ExchangeMessage {
                session: "session",
                id: "id",
                bytes: b"payload",
            },
            "exchange",
            Instant::now() + Duration::from_secs(600),
            d,
        )
        .await
    }
    #[tokio::test(start_paused = true)]
    async fn recovery_drives_both_owners_without_resubmission() {
        let mut a = ScriptedOwner::new(&[Turn::Transport("subscription_sync"), Turn::Good]);
        let mut b =
            ScriptedOwner::new(&[Turn::Transport("transport_recovery_pending"), Turn::Good]);
        a.ready_after = 2;
        b.ready_after = 2;
        let mut d = Diagnostic::default();
        scripted_exchange(&mut a, &mut b, &mut d).await.unwrap();
        assert_eq!((a.ticks.len(), b.ticks.len()), (2, 2));
        assert_eq!(
            a.sends,
            vec![
                b"payload".to_vec(),
                b"payload".to_vec(),
                b"payload\x01".to_vec()
            ]
        );
        assert!(b.sends.is_empty());
        assert_eq!(d.last_recoverable.as_ref().unwrap().role, "sender");
        assert_eq!(
            d.last_recoverable.as_ref().unwrap().label,
            "subscription_sync"
        );
    }
    #[tokio::test(start_paused = true)]
    async fn new_drivers_clear_prior_recovery_but_retain_current_recovery() {
        let previous = || Diagnostic {
            operation: "sender_tick",
            last_recoverable: Some(Recoverable {
                role: "sender",
                label: "notification_loss",
            }),
            terminal_label: None,
        };
        let mut a = ScriptedOwner::new(&[]);
        let mut b = ScriptedOwner::new(&[]);
        let mut diagnostic = previous();
        drive_until(
            &mut a,
            &mut b,
            Awaiting::Join { session: "session" },
            Instant::now() + Duration::from_secs(90),
            &mut diagnostic,
        )
        .await
        .unwrap();
        assert!(diagnostic.last_recoverable.is_none());
        diagnostic = previous();
        scripted_exchange(&mut a, &mut b, &mut diagnostic)
            .await
            .unwrap();
        assert!(diagnostic.last_recoverable.is_none());
        let mut a = ScriptedOwner::new(&[]);
        a.send_errors
            .push_back(Some(Error::Transport("relay_reconnect")));
        diagnostic = previous();
        assert!(
            scripted_exchange(&mut a, &mut b, &mut diagnostic)
                .await
                .is_err()
        );
        assert!(diagnostic.last_recoverable.is_none());
        let mut a = ScriptedOwner::new(&[Turn::Transport("subscription_sync")]);
        let mut b = ScriptedOwner::new(&[]);
        diagnostic = previous();
        drive_until(
            &mut a,
            &mut b,
            Awaiting::Join { session: "session" },
            Instant::now() + Duration::from_secs(90),
            &mut diagnostic,
        )
        .await
        .unwrap();
        let current = diagnostic.last_recoverable.unwrap();
        assert_eq!(
            (current.role, current.label),
            ("sender", "subscription_sync")
        );
    }
    #[tokio::test(start_paused = true)]
    async fn persistent_timeouts_stop_at_original_120_seconds() {
        let start = Instant::now();
        let mut a = ScriptedOwner::new(&[]);
        let mut b = ScriptedOwner::new(&[]);
        a.default = Turn::Timeout;
        b.default = Turn::Timeout;
        a.ready_after = usize::MAX;
        b.ready_after = usize::MAX;
        let mut d = Diagnostic::default();
        assert!(matches!(
            scripted_exchange(&mut a, &mut b, &mut d).await,
            Err(Error::Timeout)
        ));
        assert_eq!(Instant::now() - start, Duration::from_secs(120));
        assert_eq!(a.sends.len(), 1);
        assert!(a.ticks.len() > 1 && b.ticks.len() > 1);
        for (began, deadline) in a.ticks.iter().chain(&b.ticks) {
            assert!(*deadline <= start + Duration::from_secs(120));
            assert!(*deadline - *began <= Duration::from_secs(15));
        }
        assert_eq!(d.last_recoverable.as_ref().unwrap().label, "timeout");
        assert!(matches!(d.operation, "receiver_tick" | "sender_tick"));
    }
    #[tokio::test(start_paused = true)]
    async fn fatal_tick_stops_before_other_owner() {
        let mut a = ScriptedOwner::new(&[]);
        let mut b = ScriptedOwner::new(&[Turn::Fatal]);
        let mut d = Diagnostic::default();
        assert!(matches!(
            scripted_exchange(&mut a, &mut b, &mut d).await,
            Err(Error::Storage("scripted"))
        ));
        assert!(a.ticks.is_empty());
        assert_eq!(a.sends.len(), 1);
        assert_eq!(d.operation, "receiver_tick");
    }
    #[tokio::test(start_paused = true)]
    async fn all_terminal_transport_labels_stop_and_are_preserved() {
        for label in [
            "tls_verification",
            "tls_hostname",
            "proxy_forbidden",
            "arti_configuration",
            "relay_not_configured",
            "shutdown",
            "delivery_closed",
            "notification_forwarder_stopped",
        ] {
            let mut a = ScriptedOwner::new(&[]);
            let mut b = ScriptedOwner::new(&[Turn::Transport(label)]);
            let mut d = Diagnostic::default();
            let error = scripted_exchange(&mut a, &mut b, &mut d)
                .await
                .err()
                .unwrap();
            d.failure(&error);
            assert_eq!(d.terminal_label, Some(label));
            assert!(a.ticks.is_empty());
        }
    }
    #[tokio::test(start_paused = true)]
    async fn initial_and_duplicate_send_errors_are_terminal() {
        for (errors, operation, count) in [
            (
                vec![Some(Error::Transport("private marker"))],
                "initial_send",
                1,
            ),
            (vec![None, Some(Error::Timeout)], "duplicate_send", 2),
            (
                vec![None, None, Some(Error::Transport("relay_reconnect"))],
                "conflict_send",
                3,
            ),
        ] {
            let mut a = ScriptedOwner::new(&[]);
            let mut b = ScriptedOwner::new(&[]);
            a.send_errors = errors.into();
            let mut d = Diagnostic::default();
            assert!(scripted_exchange(&mut a, &mut b, &mut d).await.is_err());
            assert_eq!(a.sends.len(), count);
            assert_eq!(d.operation, operation);
            if count == 1 {
                assert!(a.ticks.is_empty() && b.ticks.is_empty());
            }
        }
    }
    #[tokio::test(start_paused = true)]
    async fn join_uses_same_turns_and_receiver_means_joiner() {
        let mut creator = ScriptedOwner::new(&[]);
        let mut joiner =
            ScriptedOwner::new(&[Turn::Transport("unknown private marker"), Turn::Good]);
        joiner.ready_after = 2;
        let mut d = Diagnostic::default();
        let start = Instant::now();
        drive_until(
            &mut creator,
            &mut joiner,
            Awaiting::Join { session: "session" },
            start + Duration::from_secs(90),
            &mut d,
        )
        .await
        .unwrap();
        assert_eq!((creator.ticks.len(), joiner.ticks.len()), (2, 2));
        assert!(creator.sends.is_empty() && joiner.sends.is_empty());
        assert_eq!(d.last_recoverable.as_ref().unwrap().role, "receiver");
        assert_eq!(d.last_recoverable.as_ref().unwrap().label, "other");
        assert_eq!(d.operation, "join");
    }
    #[tokio::test(start_paused = true)]
    async fn join_deadline_and_both_byte_and_receipt_proofs_are_required() {
        let mut a = ScriptedOwner::new(&[]);
        let mut b = ScriptedOwner::new(&[]);
        b.ready_after = usize::MAX;
        let start = Instant::now();
        let mut d = Diagnostic::default();
        assert!(matches!(
            drive_until(
                &mut a,
                &mut b,
                Awaiting::Join { session: "s" },
                start + Duration::from_secs(90),
                &mut d
            )
            .await,
            Err(Error::Timeout)
        ));
        assert_eq!(Instant::now() - start, Duration::from_secs(90));
        let mut a = ScriptedOwner::new(&[]);
        let mut b = ScriptedOwner::new(&[]);
        b.bytes = b"wrong".to_vec();
        assert!(matches!(
            scripted_exchange(&mut a, &mut b, &mut d).await,
            Err(Error::Mdk("byte_preservation"))
        ));
        assert_eq!(a.sends.len(), 1);
        let mut a = ScriptedOwner::new(&[]);
        let mut b = ScriptedOwner::new(&[]);
        a.ready_after = usize::MAX;
        assert!(matches!(
            scripted_exchange(&mut a, &mut b, &mut d).await,
            Err(Error::Timeout)
        ));
        assert_eq!(a.sends.len(), 1);
    }
    #[test]
    fn diagnostic_schema_never_serializes_unknown_text() {
        let mut d = Diagnostic::default();
        assert_eq!(
            serde_json::to_value(&d).unwrap(),
            serde_json::json!({"operation":"none","last_recoverable":null,"terminal_label":null})
        );
        d.operation = "initial_send";
        d.failure(&Error::Transport("private relay URL"));
        assert_eq!(
            serde_json::to_value(d).unwrap(),
            serde_json::json!({"operation":"initial_send","last_recoverable":null,"terminal_label":"other"})
        );
    }
    #[tokio::test]
    async fn partial_profile_cleanup_is_explicit_and_sticky() {
        let root = tempfile::tempdir().unwrap();
        std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let mut pair = Pair::default();
        let mut profile = ProfileLease::acquire(root.path(), "alice").unwrap();
        profile
            .prepare(
                BackendChoice::File,
                &std::env::current_exe().unwrap(),
                std::time::Instant::now() + Duration::from_secs(1),
            )
            .unwrap();
        let runtime = profile.runtime_dir().to_owned();
        let extra = runtime.join("unexpected");
        std::fs::write(&extra, b"keep").unwrap();
        pair.alice = Some(profile);
        assert!(!pair.cleanup(Instant::now() + Duration::from_secs(1)).await);
        assert!(!pair.cleanup(Instant::now() + Duration::from_secs(1)).await);
        assert!(extra.exists());
        std::fs::remove_file(extra).unwrap();
        let profile = ProfileLease::acquire(root.path(), "alice").unwrap();
        profile.cleanup_runtime().unwrap();
        assert!(!runtime.exists());
    }
    #[tokio::test]
    async fn empty_and_partial_pairs_cleanup_without_network() {
        let mut pair = Pair::default();
        assert!(pair.cleanup(Instant::now()).await);
        let root = tempfile::tempdir().unwrap();
        std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let mut profile = ProfileLease::acquire(root.path(), "alice").unwrap();
        profile
            .prepare(
                BackendChoice::File,
                &std::env::current_exe().unwrap(),
                std::time::Instant::now() + Duration::from_secs(1),
            )
            .unwrap();
        let runtime = profile.runtime_dir().to_owned();
        pair.alice = Some(profile);
        assert!(pair.cleanup(Instant::now() + Duration::from_secs(1)).await);
        assert!(!runtime.exists());
    }
    #[test]
    fn report_categories_are_finite() {
        assert_eq!(category(&Ok(Ok(()))), None);
        assert_eq!(
            category(&Ok(Err(Error::Identity("private marker")))),
            Some("identity")
        );
        assert_eq!(
            category(&Ok(Err(Error::Transport("private marker")))),
            Some("transport")
        );
        assert_eq!(category(&Ok(Err(Error::Timeout))), Some("deadline"));
    }
    #[test]
    fn runtime_recovery_cleans_failed_open_and_refuses_live_lease() {
        let root = tempfile::tempdir().unwrap();
        std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let mut profile = ProfileLease::acquire(root.path(), "alice").unwrap();
        profile
            .prepare(
                BackendChoice::File,
                &std::env::current_exe().unwrap(),
                std::time::Instant::now() + Duration::from_secs(1),
            )
            .unwrap();
        let runtime = profile.runtime_dir().to_owned();
        assert!(!recover_runtime_paths(root.path()));
        assert!(runtime.exists());
        drop(profile);
        assert!(recover_runtime_paths(root.path()));
        assert!(!runtime.exists());
    }
}
