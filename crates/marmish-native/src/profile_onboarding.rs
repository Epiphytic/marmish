use crate::command_config::CommandConfig;
use crate::identity::{BackendChoice, ProfileLease};
use crate::mdk_adapter::connect_configured_relay;
use crate::profile_directory::{self, DirectoryId};
use crate::profile_metadata::{self, MetadataSnapshot, PublicationReport};
use crate::transport::{ArtiDirs, RelayConnector, RelayPolicy};
use crate::{Error, Result};
use nostr::key::{Keys, PublicKey};
use nostr::nips::nip19::ToBech32;
use nostr_sdk::client::Client;
use std::fs::DirBuilder;
use std::os::unix::fs::DirBuilderExt;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::{Instant, timeout_at};

struct ProfileNetwork {
    connector: RelayConnector,
    _temporary: Option<tempfile::TempDir>,
}

impl ProfileNetwork {
    async fn start(policy: RelayPolicy, profile: &ProfileLease) -> Result<Self> {
        let temporary = if policy.needs_tor() && !profile.setup_snapshot().ready {
            Some(
                tempfile::Builder::new()
                    .prefix("marmish-directory-")
                    .tempdir()?,
            )
        } else {
            None
        };
        let dirs = if let Some(root) = &temporary {
            let root = root.path().canonicalize()?;
            let state = root.join("arti-state");
            let cache = root.join("arti-cache");
            DirBuilder::new().mode(0o700).create(&state)?;
            DirBuilder::new().mode(0o700).create(&cache)?;
            ArtiDirs { state, cache }
        } else {
            ArtiDirs {
                state: profile.arti_state().into(),
                cache: profile.arti_cache().into(),
            }
        };
        let network = Self {
            connector: RelayConnector::new(policy, dirs)?,
            _temporary: temporary,
        };
        network
            .connector
            .bootstrap(Instant::now() + network.connector.policy().limits().bootstrap_timeout)
            .await?;
        Ok(network)
    }

    fn sdk_client(&self, signer: Arc<Keys>) -> Result<Client> {
        #[cfg(test)]
        if let Some(client) = tests::fixture_client(&signer) {
            return Ok(client);
        }
        self.connector.sdk_client(signer)
    }

    async fn connect(&self, signer: Arc<Keys>, require_all: bool) -> Result<Client> {
        let client = self.sdk_client(signer)?;
        let limits = self.connector.policy().limits();
        let mut budget = limits.handshake_timeout;
        if self.connector.policy().needs_tor() {
            budget += limits.tor_connect_timeout;
        }
        let deadline = Instant::now() + budget;
        let results = futures_util::future::join_all(
            self.connector
                .policy()
                .urls()
                .iter()
                .map(|url| connect_configured_relay(&self.connector, &client, url, deadline)),
        )
        .await;
        if results.iter().all(Result::is_err) || (require_all && results.iter().any(Result::is_err))
        {
            let _ = close_client(&client).await;
            return Err(Error::Transport("profile_relays_unavailable"));
        }
        Ok(client)
    }

    async fn read(&self, author: PublicKey, require_all: bool) -> Result<MetadataSnapshot> {
        let reader = self
            .connect(Arc::new(Keys::generate()), require_all)
            .await?;
        let result = self.acquire(&reader, author).await;
        let close = close_client(&reader).await;
        result.and_then(|value| close.map(|()| value))
    }

    async fn acquire(&self, client: &Client, author: PublicKey) -> Result<MetadataSnapshot> {
        profile_metadata::acquire(
            client,
            self.connector.policy(),
            author,
            Instant::now() + Duration::from_secs(15),
        )
        .await
    }

    async fn mapped_key(&self, id: &DirectoryId) -> Result<PublicKey> {
        #[cfg(test)]
        if let Some(key) = tests::fixture_mapping(id) {
            return Ok(key);
        }
        let account = profile_directory::resolve(&self.connector, id).await?;
        PublicKey::from_slice(&account).map_err(|_| Error::Invalid("profile_directory_key"))
    }

    async fn close(&self) -> Result<()> {
        self.connector
            .shutdown(Instant::now() + Duration::from_secs(5))
            .await
    }
}

impl Drop for ProfileNetwork {
    fn drop(&mut self) {
        self.connector.cancel();
    }
}

async fn close_client(client: &Client) -> Result<()> {
    timeout_at(Instant::now() + Duration::from_secs(5), client.shutdown())
        .await
        .map_err(|_| Error::Transport("profile_relay_shutdown"))?;
    Ok(())
}

pub(crate) struct Candidate {
    pub(crate) npub: String,
    pub(crate) relays_queried: usize,
    pub(crate) relays_complete: usize,
}

pub(crate) async fn lookup(
    profile: &ProfileLease,
    policy: &RelayPolicy,
    id: &DirectoryId,
) -> Result<Candidate> {
    let network = ProfileNetwork::start(policy.clone(), profile).await?;
    let result = async {
        let key = network.mapped_key(id).await?;
        let metadata = network.read(key, false).await?;
        metadata.verify_nip05(&id.to_string())?;
        Ok(Candidate {
            npub: key
                .to_bech32()
                .map_err(|_| Error::Invalid("profile_directory_key"))?,
            relays_queried: metadata.coverage.queried,
            relays_complete: metadata.coverage.complete,
        })
    }
    .await;
    let close = network.close().await;
    result.and_then(|value| close.map(|()| value))
}

pub(crate) struct PublishOptions {
    pub(crate) state_dir: PathBuf,
    pub(crate) identity: String,
    pub(crate) backend: BackendChoice,
    pub(crate) helper: PathBuf,
    pub(crate) relays: Vec<String>,
    pub(crate) tor: Option<String>,
    pub(crate) nip05: DirectoryId,
    pub(crate) email: Option<String>,
    pub(crate) confirm_public: bool,
}

pub(crate) async fn publish(o: PublishOptions) -> Result<PublicationReport> {
    let open_tty = crate::native_setup::Tty::open;
    #[cfg(test)]
    let open_tty = || {
        if tests::fixture_active() {
            return Err(Error::Invalid("no_tty"));
        }
        open_tty()
    };
    let mut tty = match open_tty() {
        Ok(tty) => Some(tty),
        Err(Error::Invalid("no_tty")) if o.confirm_public => None,
        Err(error) => return Err(error),
    };
    if let Some(email) = &o.email {
        profile_directory::validate_email(email)?;
    }
    let profile = ProfileLease::acquire_ready(&o.state_dir, &o.identity)?;
    let expected = profile
        .public_key()
        .ok_or(Error::Identity("invalid_manifest"))?;
    let config = CommandConfig::load(profile.root())?;
    let configured = config.relay_policy();
    let relays = if o.relays.is_empty() {
        configured.urls().iter().map(ToString::to_string).collect()
    } else {
        o.relays
    };
    let mode = o
        .tor
        .as_deref()
        .map(str::parse)
        .transpose()?
        .unwrap_or(configured.mode());
    let policy = RelayPolicy::parse(&relays, mode, configured.limits())?;
    let network = ProfileNetwork::start(policy, &profile).await?;
    let result = async {
        if network.mapped_key(&o.nip05).await? != expected {
            return Err(Error::Invalid("profile_directory_key_mismatch"));
        }
        let base = network.read(expected, true).await?;
        let publication = base.prepare_publication(&o.nip05.to_string(), o.email.as_deref())?;
        let preview = format!(
            "Public Nostr profile changes for {} on the configured relays. Anyone can copy these fields.\n{}\n",
            expected.to_bech32().map_err(|_| Error::Identity("public_key"))?,
            publication.preview(),
        );
        if let Some(tty) = &mut tty {
            if tty.line(&format!("{preview}Type publish to continue: "))? != "publish" {
                return Err(Error::Invalid("profile_publication_unconfirmed"));
            }
        } else {
            eprint!("{preview}");
        }
        if network.mapped_key(&o.nip05).await? != expected {
            return Err(Error::Invalid("profile_directory_key_mismatch"));
        }
        let (profile, signer) = tokio::task::spawn_blocking(move || {
            let mut profile = profile;
            let material = profile.prepare(
                o.backend,
                &o.helper,
                std::time::Instant::now() + Duration::from_secs(30),
            )?;
            Ok::<_, Error>((profile, material.signer()))
        })
        .await
        .map_err(|_| Error::Identity("initialization_worker"))??;
        let publisher = network.connect(signer.clone(), true).await?;
        let sent = async {
            let fresh = network.acquire(&publisher, expected).await?;
            let event = publication.sign(&fresh, &signer)?;
            profile_metadata::publish(
                &publisher,
                network.connector.policy(),
                &event,
                Instant::now() + Duration::from_secs(15),
            )
            .await
        }
        .await;
        let close = close_client(&publisher).await;
        drop(profile);
        // Once a publish has returned a disposition, preserve it for the operator.
        if sent.is_ok() && close.is_err() {
            eprintln!("marmish: metadata relay shutdown incomplete after publication attempt");
        }
        sent
    }
    .await;
    let close = network.close().await;
    if result.is_ok() && close.is_err() {
        eprintln!("marmish: metadata connector shutdown incomplete after publication attempt");
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native_setup::{self, SetupOptions, SetupRole};
    use crate::profile_metadata::relay_tests::Fixture;
    use nostr::event::{EventBuilder, FinalizeEvent, Kind, Tag};
    use nostr::types::{Timestamp, Url};
    use nostr_sdk::prelude::SignerAuthenticator;
    use nostr_sdk::transport::websocket::{
        DefaultWebsocketTransport, WebSocketSink, WebSocketStream, WebSocketTransport,
    };
    use std::cell::RefCell;
    use std::collections::VecDeque;
    use std::net::SocketAddr;
    use std::sync::atomic::Ordering;

    const CONFIGURED_RELAY: &str = "wss://relay.invalid/";
    const IDENTIFIER: &str = "alice@example.com";

    #[derive(Clone, Debug)]
    struct LoopbackTransport {
        configured: Url,
        loopback: Url,
    }

    impl WebSocketTransport for LoopbackTransport {
        fn support_ping(&self) -> bool {
            true
        }

        fn connect<'a>(
            &'a self,
            url: &'a Url,
            proxy: Option<SocketAddr>,
        ) -> futures_util::future::BoxFuture<
            'a,
            std::result::Result<(WebSocketSink, WebSocketStream), nostr_sdk::error::Error>,
        > {
            assert_eq!(url, &self.configured);
            assert!(proxy.is_none());
            DefaultWebsocketTransport.connect(&self.loopback, None)
        }
    }

    struct PublicationFixture {
        transport: LoopbackTransport,
        directory: VecDeque<PublicKey>,
        signers: Vec<PublicKey>,
    }

    thread_local! {
        static FIXTURE: RefCell<Option<PublicationFixture>> = const { RefCell::new(None) };
    }

    pub(super) fn fixture_active() -> bool {
        FIXTURE.with(|fixture| fixture.borrow().is_some())
    }

    pub(super) fn fixture_mapping(id: &DirectoryId) -> Option<PublicKey> {
        FIXTURE.with(|fixture| {
            fixture.borrow_mut().as_mut().map(|fixture| {
                assert_eq!(id.to_string(), IDENTIFIER);
                fixture
                    .directory
                    .pop_front()
                    .expect("unexpected directory lookup")
            })
        })
    }

    pub(super) fn fixture_client(signer: &Keys) -> Option<Client> {
        FIXTURE.with(|fixture| {
            fixture.borrow_mut().as_mut().map(|fixture| {
                fixture.signers.push(signer.public_key());
                Client::builder()
                    .websocket_transport(fixture.transport.clone())
                    .authenticator(SignerAuthenticator::new(signer.clone()))
                    .build()
            })
        })
    }

    #[tokio::test(flavor = "current_thread")]
    async fn headless_publication_checks_directory_and_uses_the_local_signer() {
        let home = std::env::var_os("HOME").unwrap();
        let root = tempfile::Builder::new()
            .prefix("marmish-publication-test-")
            .tempdir_in(home)
            .unwrap();
        let state = root.path().join("state");
        let deadline = Instant::now() + Duration::from_secs(40);
        timeout_at(deadline, async {
            native_setup::run(SetupOptions {
                state_dir: state.clone(),
                identity: "default".into(),
                backend: BackendChoice::File,
                helper: PathBuf::new(),
                interactive: false,
                role: Some(SetupRole::Client),
                relays: Some(vec![CONFIGURED_RELAY.into()]),
                tor: Some("off".into()),
                grants: vec![],
                identifier: None,
                revoke: vec![],
                trust: vec![],
                untrust: vec![],
                update: false,
                no_grants: false,
                debug: false,
            })
            .await
            .unwrap();

            let seed = tokio::task::spawn_blocking({
                let state = state.clone();
                move || {
                    let mut profile = ProfileLease::acquire_ready(&state, "default").unwrap();
                    let material = profile
                        .prepare(
                            BackendChoice::File,
                            &PathBuf::new(),
                            std::time::Instant::now() + Duration::from_secs(5),
                        )
                        .unwrap();
                    EventBuilder::new(Kind::Metadata, r#"{"custom":{"visible":true}}"#)
                        .tag(Tag::parse(["x-custom", "preserved"]).unwrap())
                        .custom_created_at(Timestamp::from_secs(1))
                        .finalize(material.signer().as_ref())
                        .unwrap()
                }
            })
            .await
            .unwrap();
            let expected = seed.pubkey;
            let foreign = Keys::generate().public_key();
            let relay = Fixture::start(expected, vec![seed.clone()], true, true).await;
            FIXTURE.with(|fixture| {
                fixture.replace(Some(PublicationFixture {
                    transport: LoopbackTransport {
                        configured: Url::parse(CONFIGURED_RELAY).unwrap(),
                        loopback: Url::parse(relay.relay.as_str()).unwrap(),
                    },
                    directory: VecDeque::new(),
                    signers: vec![],
                }));
            });
            let options = |confirm_public, backend| PublishOptions {
                state_dir: state.clone(),
                identity: "default".into(),
                backend,
                helper: PathBuf::new(),
                relays: vec![],
                tor: None,
                nip05: IDENTIFIER.parse().unwrap(),
                email: Some("contact@example.com".into()),
                confirm_public,
            };
            let set_directory = |answers| {
                FIXTURE.with(|fixture| {
                    let mut fixture = fixture.borrow_mut();
                    let fixture = fixture.as_mut().unwrap();
                    assert!(fixture.directory.is_empty());
                    fixture.directory = answers;
                });
            };

            // The wrong backend would fail if publication prepared credentials.
            assert!(matches!(
                publish(options(false, BackendChoice::Keychain)).await,
                Err(Error::Invalid("no_tty"))
            ));
            set_directory(VecDeque::from([foreign]));
            assert!(matches!(
                publish(options(true, BackendChoice::Keychain)).await,
                Err(Error::Invalid("profile_directory_key_mismatch"))
            ));
            assert_eq!(relay.queries.load(Ordering::Relaxed), 0);
            assert!(relay.publications.lock().unwrap().is_empty());

            set_directory(VecDeque::from([expected, foreign]));
            assert!(matches!(
                publish(options(true, BackendChoice::Keychain)).await,
                Err(Error::Invalid("profile_directory_key_mismatch"))
            ));
            assert_eq!(relay.queries.load(Ordering::Relaxed), 1);
            assert!(relay.publications.lock().unwrap().is_empty());

            set_directory(VecDeque::from([expected, expected]));
            let report = publish(options(true, BackendChoice::File)).await.unwrap();
            assert_eq!((report.acknowledged, report.queried), (1, 1));
            assert_eq!(relay.queries.load(Ordering::Relaxed), 3);
            let fixture = FIXTURE.with(|fixture| fixture.take()).unwrap();
            assert!(fixture.directory.is_empty());
            let [first_reader, second_reader, publisher] = fixture.signers[..] else {
                panic!("expected two disposable readers and a publisher");
            };
            assert_ne!(first_reader, expected);
            assert_ne!(second_reader, expected);
            assert_eq!(publisher, expected);

            let events = relay.publications.lock().unwrap();
            let [event] = &events[..] else {
                panic!("expected exactly one published event");
            };
            assert_eq!(event.pubkey, expected);
            assert_eq!(event.kind, Kind::Metadata);
            assert_eq!(event.tags, seed.tags);
            assert!(event.verify().is_ok());
            assert!(event.created_at > seed.created_at);
            assert_eq!(
                serde_json::from_str::<serde_json::Value>(&event.content).unwrap(),
                serde_json::json!({
                    "custom":{"visible":true},
                    "email":"contact@example.com",
                    "nip05":IDENTIFIER
                })
            );
        })
        .await
        .expect("publication fixture deadline exceeded");
    }
}
