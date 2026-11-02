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

async fn exchange(
    sender: &mut MdkOwner,
    receiver: &mut MdkOwner,
    session: &str,
    id: &str,
    bytes: &[u8],
    stage: &'static str,
    overall: Instant,
) -> Result<Sample> {
    let start = Instant::now();
    let deadline = overall.min(start + Duration::from_secs(120));
    sender.send(session, id, bytes, deadline).await?;
    loop {
        if Instant::now() >= deadline {
            return Err(Error::Timeout);
        }
        receiver.tick(deadline).await?;
        sender.tick(deadline).await?;
        let matching = receiver
            .received()?
            .into_iter()
            .find(|m| m.session == session && m.application_id == id && !m.invalidated);
        if let Some(message) = matching {
            if message.data != bytes {
                return Err(Error::Mdk("byte_preservation"));
            }
            if sender.sent_status(session, id)?.state == "received" {
                // Identical retry must return the original intent; changed bytes
                // under the same application ID must never become a second send.
                sender.send(session, id, bytes, deadline).await?;
                let mut changed = bytes.to_vec();
                changed.push(1);
                if sender.send(session, id, &changed, deadline).await.is_ok() {
                    return Err(Error::Mdk("conflicting_application_id_accepted"));
                }
                return Ok(Sample {
                    stage,
                    milliseconds: start.elapsed().as_millis(),
                    bytes: bytes.len(),
                    sha256: hex::encode(Sha256::digest(bytes)),
                    bytes_per_second: Some(
                        ((bytes.len() as u128) * 1_000_000_000 / start.elapsed().as_nanos().max(1))
                            as u64,
                    ),
                });
            }
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

async fn execute(
    pair: &mut Pair,
    root: &Path,
    relays: &[String],
    mode: RouteMode,
    samples: &mut Vec<Sample>,
    stage: &mut &'static str,
    overall: Instant,
) -> Result<()> {
    crate::diagnostics::stage(stage, "identity_prepare");
    create_pair(pair, root.to_owned(), relays, mode).await?;
    let (a, b) = (pair.a.as_mut().unwrap(), pair.b.as_mut().unwrap());
    crate::diagnostics::stage(stage, "activation_cold");
    let began = Instant::now();
    a.activate(overall.min(began + Duration::from_secs(150)))
        .await?;
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
    let conversation = a
        .open_conversation(
            &b_key,
            overall.min(Instant::now() + Duration::from_secs(30)),
        )
        .await?;
    crate::diagnostics::stage(stage, "conversation_join");
    let join_deadline = overall.min(Instant::now() + Duration::from_secs(90));
    loop {
        if Instant::now() >= join_deadline {
            return Err(Error::Timeout);
        }
        b.tick(join_deadline).await?;
        a.tick(join_deadline).await?;
        if b.conversations()?
            .iter()
            .any(|c| c.session == conversation.session && !c.revoked)
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let edge=b"\0\x01\x7f\xff quotes:'\" backslash:\\ dollar:$ newline:\n cr:\r tab:\t utf8:\xf0\x9f\xa6\xab";
    crate::diagnostics::stage(stage, "alice_to_bob");
    samples.push(
        exchange(
            a,
            b,
            &conversation.session,
            &crate::foundation::fresh_id(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_err(|_| Error::Invalid("clock"))?
                    .as_secs(),
            )?,
            edge,
            stage,
            overall,
        )
        .await?,
    );
    crate::diagnostics::stage(stage, "bob_to_alice");
    samples.push(
        exchange(
            b,
            a,
            &conversation.session,
            &crate::foundation::fresh_id(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_err(|_| Error::Invalid("clock"))?
                    .as_secs(),
            )?,
            edge,
            stage,
            overall,
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
    let began = Instant::now();
    let (profile, owner) = daemon::open(config(root, "alice", relays, mode, b_key)?).await?;
    pair.alice = Some(profile);
    pair.a = Some(owner);
    let (profile, owner) = daemon::open(config(root, "bob", relays, mode, a_key)?).await?;
    pair.bob = Some(profile);
    pair.b = Some(owner);
    let (a, b) = (pair.a.as_mut().unwrap(), pair.b.as_mut().unwrap());
    crate::diagnostics::stage(stage, "restart_activation_warm");
    a.activate(overall.min(Instant::now() + Duration::from_secs(150)))
        .await?;
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
            &conversation.session,
            &crate::foundation::fresh_id(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_err(|_| Error::Invalid("clock"))?
                    .as_secs(),
            )?,
            edge,
            stage,
            overall,
        )
        .await?,
    );
    crate::diagnostics::stage(stage, "restart_bob_to_alice");
    samples.push(
        exchange(
            b,
            a,
            &conversation.session,
            &crate::foundation::fresh_id(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_err(|_| Error::Invalid("clock"))?
                    .as_secs(),
            )?,
            edge,
            stage,
            overall,
        )
        .await?,
    );
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
    let result = tokio::time::timeout_at(
        execution,
        execute(
            &mut pair,
            root.path(),
            &relays,
            mode,
            &mut samples,
            &mut stage,
            execution,
        ),
    )
    .await;
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
        serde_json::json!({"schema":1,"protocol":crate::foundation::PROTOCOL,"phase":"v0.2-phase1","stage":stage,"route":route,"ok":category.is_none()&&cleanup_ok,"error":category,"category":category,"target":std::env::consts::ARCH,"platform":std::env::consts::OS,"relay_count":relays.len(),"elapsed_ms":start.elapsed().as_millis(),"samples":samples,"cleanup_ok":cleanup_ok,"runtime_cleanup_ok":runtime_cleanup,"state_cleanup_ok":state_cleanup}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
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
