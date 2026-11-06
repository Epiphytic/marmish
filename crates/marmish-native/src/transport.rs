//! Explicit relay authority and a native direct/Arti WebSocket connector.
//!
//! Construction is configuration-only. The owner must open encrypted state
//! before calling `bootstrap`; no SDK default connector or proxy is used.

use crate::{Error, Result};
use arti_client::{BootstrapBehavior, TorClient, config::TorClientConfigBuilder};
use async_wsocket::Message;
use futures_util::{SinkExt, StreamExt};
use nostr::types::Url;
use nostr_sdk::prelude::SignerAuthenticator;
use nostr_sdk::transport::websocket::{WebSocketSink, WebSocketStream, WebSocketTransport};
use sha3::{Digest, Sha3_256};
use std::collections::BTreeSet;
use std::fmt;
use std::net::SocketAddr;
use std::num::NonZeroUsize;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::{Mutex, Semaphore};
use tokio::time::{Instant, timeout_at};
use tokio_tungstenite::tungstenite::{Message as NativeMessage, protocol::WebSocketConfig};
use tokio_util::sync::CancellationToken;
use tor_rtcompat::PreferredRuntime;

/// Initial relay set for new profiles; persisted configuration remains authoritative.
pub(crate) fn default_relays() -> Vec<String> {
    vec![
        "wss://relay.eu.whitenoise.chat/".into(),
        "wss://relay.us.whitenoise.chat/".into(),
    ]
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RouteMode {
    /// Onion relays through Tor; ordinary configured WSS relays directly.
    Auto,
    /// Every configured relay through Tor.
    Always,
    /// Direct WSS only; onion destinations are rejected during parsing.
    Off,
}

impl FromStr for RouteMode {
    type Err = Error;
    fn from_str(value: &str) -> Result<Self> {
        match value {
            "auto" => Ok(Self::Auto),
            "always" => Ok(Self::Always),
            "off" => Ok(Self::Off),
            _ => Err(Error::Invalid("route_mode")),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct TransportLimits {
    pub max_relays: usize,
    pub max_frame_bytes: usize,
    pub queue_capacity: usize,
    pub handshake_timeout: Duration,
    pub tor_connect_timeout: Duration,
    pub bootstrap_timeout: Duration,
}

impl Default for TransportLimits {
    fn default() -> Self {
        Self {
            max_relays: 16,
            max_frame_bytes: 1024 * 1024,
            queue_capacity: 64,
            handshake_timeout: Duration::from_secs(15),
            tor_connect_timeout: Duration::from_secs(120),
            bootstrap_timeout: Duration::from_secs(120),
        }
    }
}

impl TransportLimits {
    fn validate(self) -> Result<Self> {
        if !(1..=16).contains(&self.max_relays)
            || !(1024..=1024 * 1024).contains(&self.max_frame_bytes)
            || !(1..=64).contains(&self.queue_capacity)
            || self.handshake_timeout.is_zero()
            || self.handshake_timeout > Duration::from_secs(15)
            || self.tor_connect_timeout.is_zero()
            || self.tor_connect_timeout > Duration::from_secs(120)
            || self.bootstrap_timeout.is_zero()
            || self.bootstrap_timeout > Duration::from_secs(120)
        {
            return Err(Error::Invalid("transport_limits"));
        }
        Ok(self)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Route {
    Direct,
    Tor,
}

#[derive(Clone, Debug)]
pub struct RelayPolicy {
    urls: Vec<Url>,
    routes: Vec<Route>,
    mode: RouteMode,
    limits: TransportLimits,
}

impl RelayPolicy {
    pub fn parse(urls: &[String], mode: RouteMode, limits: TransportLimits) -> Result<Self> {
        let limits = limits.validate()?;
        if urls.is_empty() || urls.len() > limits.max_relays {
            return Err(Error::Invalid("relay_count"));
        }
        let mut canonical = BTreeSet::new();
        for input in urls {
            canonical.insert(canonical_url(input)?);
        }
        let mut urls = Vec::new();
        let mut routes = Vec::new();
        for value in canonical {
            let parsed = url::Url::parse(&value).map_err(|_| Error::Invalid("relay_url"))?;
            let host = parsed.host_str().ok_or(Error::Invalid("relay_host"))?;
            let onion = host.ends_with(".onion");
            if onion && !valid_onion(host) {
                return Err(Error::Invalid("onion_address"));
            }
            let route = match (mode, onion) {
                (RouteMode::Off, true) => return Err(Error::Invalid("onion_route_off")),
                (RouteMode::Always, _) | (RouteMode::Auto, true) => Route::Tor,
                _ => Route::Direct,
            };
            if parsed.scheme() == "ws" && !(onion && route == Route::Tor) {
                return Err(Error::Invalid("plaintext_relay"));
            }
            urls.push(Url::parse(&value).map_err(|_| Error::Invalid("relay_url"))?);
            routes.push(route);
        }
        Ok(Self {
            urls,
            routes,
            mode,
            limits,
        })
    }

    pub fn urls(&self) -> &[Url] {
        &self.urls
    }
    pub fn mode(&self) -> RouteMode {
        self.mode
    }
    pub fn limits(&self) -> TransportLimits {
        self.limits
    }
    pub fn needs_tor(&self) -> bool {
        self.routes.contains(&Route::Tor)
    }

    fn destination(&self, input: &Url) -> Result<(usize, Route)> {
        let canonical = canonical_url(input.as_str())?;
        let index = self
            .urls
            .iter()
            .position(|url| url.as_str() == canonical)
            .ok_or(Error::Transport("relay_not_configured"))?;
        Ok((index, self.routes[index]))
    }
}

// Paths (including percent-encoded paths) are supported and remain exact URL
// components. Queries are deliberately unsupported: no secret query tokens.
fn canonical_url(input: &str) -> Result<String> {
    if input.is_empty()
        || input.len() > 2048
        || input
            .chars()
            .any(|ch| ch.is_control() || ch.is_whitespace() || ch == '\\')
    {
        return Err(Error::Invalid("relay_url"));
    }
    let parsed = url::Url::parse(input).map_err(|_| Error::Invalid("relay_url"))?;
    if !matches!(parsed.scheme(), "ws" | "wss")
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.fragment().is_some()
        || parsed.query().is_some()
        || parsed.port() == Some(0)
    {
        return Err(Error::Invalid("relay_components"));
    }
    let authority = input
        .split_once("://")
        .ok_or(Error::Invalid("relay_url"))?
        .1
        .split('/')
        .next()
        .ok_or(Error::Invalid("relay_url"))?;
    if authority.ends_with(':') || authority.contains('@') {
        return Err(Error::Invalid("relay_port"));
    }
    if let Some((_, port)) = authority.rsplit_once(':') {
        // A closing bracket distinguishes an IPv6 host with no explicit port.
        if !port.ends_with(']') && (port.len() > 1 && port.starts_with('0')) {
            return Err(Error::Invalid("relay_port"));
        }
    }
    let host = parsed.host_str().ok_or(Error::Invalid("relay_host"))?;
    if host.is_empty() || host.ends_with('.') || host.contains('%') {
        return Err(Error::Invalid("relay_host"));
    }
    Ok(parsed.to_string())
}

fn valid_onion(host: &str) -> bool {
    let Some(label) = host.strip_suffix(".onion") else {
        return false;
    };
    if label.len() != 56
        || !label
            .bytes()
            .all(|ch| matches!(ch, b'a'..=b'z' | b'2'..=b'7'))
    {
        return false;
    }
    let Ok(decoded) = data_encoding::BASE32_NOPAD.decode(label.to_uppercase().as_bytes()) else {
        return false;
    };
    if decoded.len() != 35 || decoded[34] != 3 {
        return false;
    }
    let mut checksum = Sha3_256::new();
    checksum.update(b".onion checksum");
    checksum.update(&decoded[..32]);
    checksum.update([3]);
    let checksum = checksum.finalize();
    decoded[32..34] == checksum[..2]
}

#[derive(Clone, Debug)]
pub struct ArtiDirs {
    pub state: PathBuf,
    pub cache: PathBuf,
}

struct ConnectorState {
    policy: RelayPolicy,
    dirs: ArtiDirs,
    tor: Mutex<Option<Arc<TorClient<PreferredRuntime>>>>,
    bootstrap_lock: Mutex<()>,
    ready: AtomicBool,
    cancelled: CancellationToken,
    slots: Vec<Arc<Semaphore>>,
    #[cfg(test)]
    direct_attempts: std::sync::atomic::AtomicUsize,
}

#[derive(Clone)]
pub struct RelayConnector(Arc<ConnectorState>);

impl fmt::Debug for RelayConnector {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        out.debug_struct("RelayConnector")
            .field("relay_count", &self.0.policy.urls.len())
            .field("mode", &self.0.policy.mode)
            .field("ready", &self.0.ready.load(Ordering::Acquire))
            .finish()
    }
}

impl RelayConnector {
    pub fn new(policy: RelayPolicy, dirs: ArtiDirs) -> Result<Self> {
        if !dirs.state.is_absolute() || !dirs.cache.is_absolute() || dirs.state == dirs.cache {
            return Err(Error::Invalid("arti_directories"));
        }
        let slots = policy
            .urls
            .iter()
            .map(|_| Arc::new(Semaphore::new(1)))
            .collect();
        Ok(Self(Arc::new(ConnectorState {
            policy,
            dirs,
            tor: Mutex::new(None),
            bootstrap_lock: Mutex::new(()),
            ready: AtomicBool::new(false),
            cancelled: CancellationToken::new(),
            slots,
            #[cfg(test)]
            direct_attempts: std::sync::atomic::AtomicUsize::new(0),
        })))
    }

    pub fn policy(&self) -> &RelayPolicy {
        &self.0.policy
    }

    /// The complete attempt budget for an explicitly configured endpoint.
    pub fn connection_timeout(&self, url: &Url) -> Result<Duration> {
        let (_, route) = self.policy().destination(url)?;
        self.route_timeout(route)
    }

    fn route_timeout(&self, route: Route) -> Result<Duration> {
        let limits = self.policy().limits;
        match route {
            Route::Direct => Ok(limits.handshake_timeout),
            Route::Tor => limits
                .tor_connect_timeout
                .checked_add(limits.handshake_timeout)
                .ok_or(Error::Invalid("transport_limits")),
        }
    }

    fn maximum_connection_timeout(&self) -> Result<Duration> {
        self.route_timeout(if self.policy().needs_tor() {
            Route::Tor
        } else {
            Route::Direct
        })
    }

    pub(crate) fn relay_limits(&self) -> nostr_sdk::prelude::RelayLimits {
        let mut limits = nostr_sdk::prelude::RelayLimits::default();
        limits.messages.max_size = Some(self.policy().limits.max_frame_bytes as u32);
        limits.events.max_size = Some(self.policy().limits.max_frame_bytes as u32);
        limits
    }

    pub fn sdk_client(&self, signer: Arc<nostr::key::Keys>) -> Result<nostr_sdk::client::Client> {
        if self.0.cancelled.is_cancelled() {
            return Err(Error::Transport("shutdown"));
        }
        let limits = self.policy().limits;
        Ok(nostr_sdk::client::Client::builder()
            .websocket_transport(self.clone())
            .authenticator(SignerAuthenticator::new(signer.as_ref().clone()))
            .max_relays(NonZeroUsize::new(limits.max_relays))
            .notification_channel_size(NonZeroUsize::new(limits.queue_capacity).expect("validated"))
            .connect_timeout(self.maximum_connection_timeout()?)
            .relay_limits(self.relay_limits())
            .build())
    }

    /// Called only after successful encrypted runtime open by the state owner.
    pub async fn bootstrap(&self, deadline: Instant) -> Result<()> {
        if deadline <= Instant::now() {
            return Err(Error::Timeout);
        }
        let until = deadline.min(Instant::now() + self.policy().limits.bootstrap_timeout);
        tokio::select! {
            biased;
            _ = self.0.cancelled.cancelled() => Err(Error::Transport("shutdown")),
            result = timeout_at(until, self.bootstrap_inner()) => result.map_err(|_| Error::Timeout)?,
        }
    }

    async fn bootstrap_inner(&self) -> Result<()> {
        let _guard = self.0.bootstrap_lock.lock().await;
        if self.0.ready.load(Ordering::Acquire) {
            return Ok(());
        }
        if self.policy().needs_tor() {
            // Arti directory caches share this process's linked SQLCipher.
            marmish_engine::initialize_storage().map_err(|_| Error::Storage("sqlcipher_init"))?;
            crate::diagnostics::event("arti_bootstrap", "begin");
            for path in [&self.0.dirs.state, &self.0.dirs.cache] {
                check_private_directory(path)?;
            }
            let config =
                TorClientConfigBuilder::from_directories(&self.0.dirs.state, &self.0.dirs.cache)
                    .build()
                    .map_err(|_| Error::Transport("arti_configuration"))?;
            // Manual mode forbids accidental on-demand bootstrap from connect.
            let client = TorClient::builder()
                .config(config)
                .bootstrap_behavior(BootstrapBehavior::Manual)
                .create_unbootstrapped_async()
                .await
                .map_err(|_| Error::Transport("arti_initialization"))?;
            client
                .bootstrap()
                .await
                .map_err(|_| Error::Transport("arti_bootstrap"))?;
            *self.0.tor.lock().await = Some(client);
            crate::diagnostics::event("arti_bootstrap", "ready");
        }
        self.0.ready.store(true, Ordering::Release);
        Ok(())
    }

    /// Terminal shutdown; dropping all owners releases Arti state and circuits.
    pub async fn shutdown(&self, deadline: Instant) -> Result<()> {
        self.cancel();
        timeout_at(deadline, async {
            self.0.tor.lock().await.take();
        })
        .await
        .map_err(|_| Error::Timeout)?;
        Ok(())
    }

    /// Synchronous terminal cancellation for an owner's error/Drop path.
    /// The owner must also shut down its SDK pool before removing profile state.
    pub fn cancel(&self) {
        self.0.cancelled.cancel();
        self.0.ready.store(false, Ordering::Release);
        for slot in &self.0.slots {
            slot.close();
        }
        // Do not block a Drop path behind an in-flight async bootstrap. That
        // operation observes the cancellation token and drops its local client.
        if let Ok(mut client) = self.0.tor.try_lock() {
            client.take();
        }
    }

    async fn open_socket(
        &self,
        url: &Url,
        proxy: Option<SocketAddr>,
    ) -> Result<(WebSocketSink, WebSocketStream)> {
        if proxy.is_some() {
            return Err(Error::Transport("proxy_forbidden"));
        }
        let (index, route) = self.policy().destination(url)?;
        if !self.0.ready.load(Ordering::Acquire) {
            return Err(Error::Transport("not_ready"));
        }
        if self.0.cancelled.is_cancelled() {
            return Err(Error::Transport("shutdown"));
        }
        self.connection_phases(
            route,
            self.connect_stream(url, index, route),
            |(stream, permit)| self.finish_socket(url, stream, permit),
        )
        .await
    }

    // The stream future includes permit acquisition. Direct retains one total
    // envelope; Tor gets a stream budget followed by one shared TLS/WS budget.
    async fn connection_phases<S, T, F, H, HF>(
        &self,
        route: Route,
        stream: F,
        handshake: H,
    ) -> Result<T>
    where
        F: std::future::Future<Output = Result<S>>,
        H: FnOnce(S) -> HF,
        HF: std::future::Future<Output = Result<T>>,
    {
        let limits = self.policy().limits;
        let deadline = Instant::now() + self.route_timeout(route)?;
        let attempt = async {
            let stream = match route {
                Route::Direct => stream.await?,
                Route::Tor => timeout_at(Instant::now() + limits.tor_connect_timeout, stream)
                    .await
                    .map_err(|_| Error::Timeout)??,
            };
            match route {
                Route::Direct => handshake(stream).await,
                Route::Tor => {
                    timeout_at(Instant::now() + limits.handshake_timeout, handshake(stream))
                        .await
                        .map_err(|_| Error::Timeout)?
                }
            }
        };
        tokio::select! {
            biased;
            _ = self.0.cancelled.cancelled() => Err(Error::Transport("shutdown")),
            result = timeout_at(deadline, attempt) => result.map_err(|_| Error::Timeout)?,
        }
    }

    async fn connect_stream(
        &self,
        url: &Url,
        index: usize,
        route: Route,
    ) -> Result<(
        Box<dyn NetworkStream>,
        Arc<tokio::sync::OwnedSemaphorePermit>,
    )> {
        // The permit lives until BOTH SDK halves are dropped. Discovery and
        // ordinary runtime cannot establish parallel connections to one relay.
        crate::diagnostics::event("relay_slot", "waiting");
        let permit = Arc::new(
            self.0.slots[index]
                .clone()
                .acquire_owned()
                .await
                .map_err(|_| Error::Transport("shutdown"))?,
        );
        let parsed = url::Url::parse(url.as_str()).map_err(|_| Error::Invalid("relay_url"))?;
        let host = parsed.host_str().ok_or(Error::Invalid("relay_host"))?;
        let host = host.trim_start_matches('[').trim_end_matches(']');
        let port = parsed
            .port_or_known_default()
            .ok_or(Error::Invalid("relay_port"))?;
        let stream: Box<dyn NetworkStream> = match route {
            Route::Tor => {
                crate::diagnostics::event("arti_connect", "begin");
                let client = self
                    .0
                    .tor
                    .lock()
                    .await
                    .clone()
                    .ok_or(Error::Transport("arti_not_ready"))?;
                // Passing the hostname untouched avoids local DNS. Errors do
                // not enter the direct branch, including ordinary WSS over Tor.
                Box::new(
                    client
                        .connect((host, port))
                        .await
                        .map_err(|_| Error::Transport("arti_connect"))?,
                )
            }
            Route::Direct => {
                crate::diagnostics::event("direct_connect", "begin");
                #[cfg(test)]
                self.0.direct_attempts.fetch_add(1, Ordering::Relaxed);
                Box::new(
                    tokio::net::TcpStream::connect((host, port))
                        .await
                        .map_err(|_| Error::Transport("direct_connect"))?,
                )
            }
        };
        crate::diagnostics::event("relay_stream", "connected");
        Ok((stream, permit))
    }

    async fn finish_socket(
        &self,
        url: &Url,
        stream: Box<dyn NetworkStream>,
        permit: Arc<tokio::sync::OwnedSemaphorePermit>,
    ) -> Result<(WebSocketSink, WebSocketStream)> {
        let parsed = url::Url::parse(url.as_str()).map_err(|_| Error::Invalid("relay_url"))?;
        let host = parsed
            .host_str()
            .ok_or(Error::Invalid("relay_host"))?
            .trim_start_matches('[')
            .trim_end_matches(']');
        let stream = if parsed.scheme() == "wss" {
            crate::diagnostics::event("tls", "begin");
            tls_stream(stream, host).await?
        } else {
            stream
        };
        self.websocket(url, stream, permit).await
    }

    async fn websocket(
        &self,
        url: &Url,
        stream: Box<dyn NetworkStream>,
        permit: Arc<tokio::sync::OwnedSemaphorePermit>,
    ) -> Result<(WebSocketSink, WebSocketStream)> {
        let max = self.policy().limits.max_frame_bytes;
        let config = WebSocketConfig::default()
            .max_message_size(Some(max))
            .max_frame_size(Some(max))
            .write_buffer_size(16 * 1024)
            .max_write_buffer_size(max + 32 * 1024)
            .read_buffer_size(16 * 1024);
        crate::diagnostics::event("websocket_handshake", "begin");
        // client_async never creates another TCP connection or follows redirects.
        let (socket, _) =
            tokio_tungstenite::client_async_with_config(url.as_str(), stream, Some(config))
                .await
                .map_err(|_| Error::Transport("websocket_handshake"))?;
        crate::diagnostics::event("websocket_handshake", "ready");
        let (sink, stream) = socket.split();
        let sink_permit = permit.clone();
        let cancelled = self.0.cancelled.clone();
        let sink =
            sink.sink_map_err(|_| sdk_error("websocket_write"))
                .with(move |message: Message| {
                    let _permit = sink_permit.clone();
                    let cancelled = cancelled.clone();
                    async move {
                        if cancelled.is_cancelled() {
                            return Err(sdk_error("shutdown"));
                        }
                        if message.len() > max {
                            return Err(sdk_error("frame_limit"));
                        }
                        Ok::<NativeMessage, nostr_sdk::error::Error>(message.into())
                    }
                });
        let stream = stream
            .take_until(self.0.cancelled.clone().cancelled_owned())
            .map(move |message| {
                let _permit = &permit;
                message
                    .map_err(|_| sdk_error("websocket_read"))
                    .and_then(from_native)
            });
        Ok((Box::pin(sink), Box::pin(stream)))
    }
}

trait NetworkStream: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> NetworkStream for T {}

async fn tls_stream(stream: Box<dyn NetworkStream>, host: &str) -> Result<Box<dyn NetworkStream>> {
    let roots = rustls::RootCertStore::from_iter(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    // Explicit provider avoids ambient process-wide provider selection.
    let config = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .map_err(|_| Error::Transport("tls_configuration"))?
    .with_root_certificates(roots)
    .with_no_client_auth();
    let name = rustls::pki_types::ServerName::try_from(host.to_owned())
        .map_err(|_| Error::Transport("tls_hostname"))?;
    let secured = tokio_rustls::TlsConnector::from(Arc::new(config))
        .connect(name, stream)
        .await
        .map_err(|_| Error::Transport("tls_verification"))?;
    Ok(Box::new(secured))
}

fn check_private_directory(path: &std::path::Path) -> Result<()> {
    use std::os::unix::fs::MetadataExt;
    // The profile owner creates these directories under its lease. Refuse
    // symlink traversal anywhere in the supplied absolute directory chain.
    let mut current = PathBuf::new();
    for component in path.components() {
        if matches!(component, std::path::Component::ParentDir) {
            return Err(Error::Invalid("arti_directories"));
        }
        current.push(component);
        let metadata = std::fs::symlink_metadata(&current)?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(Error::Invalid("arti_directories"));
        }
    }
    if std::fs::symlink_metadata(path)?.mode() & 0o077 != 0 {
        return Err(Error::Invalid("arti_directory_permissions"));
    }
    Ok(())
}

fn sdk_error(category: &'static str) -> nostr_sdk::error::Error {
    nostr_sdk::error::Error::transport(std::io::Error::other(category))
}

fn connector_error_label(error: &Error) -> &'static str {
    match error {
        Error::Timeout => "timeout",
        Error::Io(_) => "io",
        Error::Transport(label)
            if matches!(
                *label,
                "proxy_forbidden"
                    | "relay_not_configured"
                    | "not_ready"
                    | "shutdown"
                    | "arti_not_ready"
                    | "arti_connect"
                    | "direct_connect"
                    | "websocket_handshake"
                    | "tls_configuration"
                    | "tls_hostname"
                    | "tls_verification"
            ) =>
        {
            label
        }
        _ => "other",
    }
}

fn from_native(message: NativeMessage) -> std::result::Result<Message, nostr_sdk::error::Error> {
    Ok(match message {
        NativeMessage::Text(value) => Message::Text(value.to_string()),
        NativeMessage::Binary(value) => Message::Binary(value.to_vec()),
        NativeMessage::Ping(value) => Message::Ping(value.to_vec()),
        NativeMessage::Pong(value) => Message::Pong(value.to_vec()),
        NativeMessage::Close(value) => Message::Close(value.map(Into::into)),
        NativeMessage::Frame(_) => return Err(sdk_error("unexpected_frame")),
    })
}

impl WebSocketTransport for RelayConnector {
    fn support_ping(&self) -> bool {
        true
    }
    fn connect<'a>(
        &'a self,
        url: &'a Url,
        proxy: Option<SocketAddr>,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<
                    Output = std::result::Result<
                        (WebSocketSink, WebSocketStream),
                        nostr_sdk::error::Error,
                    >,
                > + Send
                + 'a,
        >,
    > {
        Box::pin(async move {
            self.open_socket(url, proxy).await.map_err(|error| {
                crate::diagnostics::event("connector_failure", connector_error_label(&error));
                sdk_error("relay_connect")
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn white_noise_defaults_are_canonical_and_follow_route_policy() {
        let relays = default_relays();
        assert_eq!(
            relays,
            [
                "wss://relay.eu.whitenoise.chat/",
                "wss://relay.us.whitenoise.chat/",
            ]
        );
        for mode in [RouteMode::Auto, RouteMode::Off, RouteMode::Always] {
            let policy = RelayPolicy::parse(&relays, mode, TransportLimits::default()).unwrap();
            assert_eq!(
                policy
                    .urls()
                    .iter()
                    .map(|url| url.as_str())
                    .collect::<Vec<_>>(),
                relays
            );
            assert_eq!(policy.needs_tor(), mode == RouteMode::Always);
            assert_eq!(
                policy.routes,
                vec![
                    if mode == RouteMode::Always {
                        Route::Tor
                    } else {
                        Route::Direct
                    };
                    2
                ]
            );
        }
        assert!(matches!(
            RelayPolicy::parse(&[], RouteMode::Auto, TransportLimits::default()),
            Err(Error::Invalid("relay_count"))
        ));
    }

    #[test]
    fn connector_diagnostics_redact_unknown_errors() {
        assert_eq!(connector_error_label(&Error::Timeout), "timeout");
        assert_eq!(
            connector_error_label(&Error::Transport("tls_verification")),
            "tls_verification"
        );
        assert_eq!(
            connector_error_label(&Error::Transport("private relay URL")),
            "other"
        );
        assert_eq!(
            connector_error_label(&Error::Io(std::io::Error::other("private path"))),
            "io"
        );
        assert_eq!(
            connector_error_label(&Error::Invalid("private input")),
            "other"
        );
    }

    fn onion() -> String {
        let key = [7_u8; 32];
        let mut hash = Sha3_256::new();
        hash.update(b".onion checksum");
        hash.update(key);
        hash.update([3]);
        let digest = hash.finalize();
        let mut bytes = key.to_vec();
        bytes.extend_from_slice(&digest[..2]);
        bytes.push(3);
        format!(
            "{}.onion",
            data_encoding::BASE32_NOPAD.encode(&bytes).to_lowercase()
        )
    }

    fn policy(urls: &[&str], mode: RouteMode) -> Result<RelayPolicy> {
        RelayPolicy::parse(
            &urls.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
            mode,
            TransportLimits::default(),
        )
    }

    fn connector(policy: RelayPolicy) -> RelayConnector {
        RelayConnector::new(
            policy,
            ArtiDirs {
                state: "/nonexistent/state".into(),
                cache: "/nonexistent/cache".into(),
            },
        )
        .unwrap()
    }

    #[test]
    fn explicit_canonical_set_and_path_authority() {
        let policy = policy(
            &[
                "wss://EXAMPLE.com:443/relay/path",
                "wss://example.com/relay/path",
            ],
            RouteMode::Auto,
        )
        .unwrap();
        assert_eq!(policy.urls.len(), 1);
        assert_eq!(policy.urls[0].as_str(), "wss://example.com/relay/path");
        assert!(
            policy
                .destination(&Url::parse("wss://example.com/other").unwrap())
                .is_err()
        );
        assert!(
            policy
                .destination(&Url::parse("wss://other.example/relay/path").unwrap())
                .is_err()
        );
        assert!(!policy.needs_tor());
    }

    #[test]
    fn rejects_malformed_or_ambient_authority() {
        for url in [
            "",
            "https://example.com",
            "ws://example.com",
            "wss://user@example.com",
            "wss://example.com/#fragment",
            "wss://example.com/?token=x",
            "wss://example.com:",
            "wss://example.com:0",
            "wss://example.com:0443",
            "wss://example.com./",
            "wss://example.com/\n",
            "wss://example.com\\evil",
            "ws://invalid.onion",
        ] {
            assert!(policy(&[url], RouteMode::Auto).is_err(), "{url:?}");
        }
        assert!(policy(&[], RouteMode::Auto).is_err());
        assert!(policy(&["wss://example.com"; 17], RouteMode::Auto).is_err());
    }

    #[test]
    fn onion_validation_and_route_modes() {
        let onion = onion();
        assert!(valid_onion(&onion));
        let ws = format!("ws://{onion}/relay");
        assert_eq!(
            policy(&[&ws], RouteMode::Auto).unwrap().routes,
            vec![Route::Tor]
        );
        assert!(policy(&[&ws], RouteMode::Off).is_err());
        assert_eq!(
            policy(&["wss://example.com"], RouteMode::Always)
                .unwrap()
                .routes,
            vec![Route::Tor]
        );
        let mut corrupted = onion.into_bytes();
        corrupted[0] = if corrupted[0] == b'a' { b'b' } else { b'a' };
        assert!(!valid_onion(std::str::from_utf8(&corrupted).unwrap()));
    }

    #[test]
    fn immutable_limits_have_hard_ceilings() {
        for limits in [
            TransportLimits {
                max_relays: 17,
                ..TransportLimits::default()
            },
            TransportLimits {
                max_frame_bytes: usize::MAX,
                ..TransportLimits::default()
            },
            TransportLimits {
                queue_capacity: 0,
                ..TransportLimits::default()
            },
            TransportLimits {
                handshake_timeout: Duration::ZERO,
                ..TransportLimits::default()
            },
        ] {
            assert!(
                RelayPolicy::parse(&["wss://example.com".into()], RouteMode::Auto, limits).is_err()
            );
        }
    }

    #[test]
    fn configured_routes_select_bounded_sdk_budgets_and_limits() {
        let onion = format!("ws://{}/", onion());
        let connector =
            connector(policy(&["wss://example.com/", &onion], RouteMode::Auto).unwrap());
        assert_eq!(
            connector
                .connection_timeout(&Url::parse("wss://example.com/").unwrap())
                .unwrap(),
            Duration::from_secs(15)
        );
        assert_eq!(
            connector
                .connection_timeout(&Url::parse(&onion).unwrap())
                .unwrap(),
            Duration::from_secs(135)
        );
        assert_eq!(
            connector.maximum_connection_timeout().unwrap(),
            Duration::from_secs(135)
        );
        assert!(
            connector
                .connection_timeout(&Url::parse("wss://unconfigured.example/").unwrap())
                .is_err()
        );
        let limits = connector.relay_limits();
        assert_eq!(limits.messages.max_size, Some(1024 * 1024));
        assert_eq!(limits.events.max_size, Some(1024 * 1024));
        for duration in [Duration::ZERO, Duration::from_secs(121)] {
            assert!(
                RelayPolicy::parse(
                    std::slice::from_ref(&onion),
                    RouteMode::Auto,
                    TransportLimits {
                        tor_connect_timeout: duration,
                        ..TransportLimits::default()
                    }
                )
                .is_err()
            );
        }
    }

    #[tokio::test(start_paused = true)]
    async fn route_phase_budgets_bound_stream_and_shared_tls_websocket() {
        // Exercise the production wrapper with inert stream/TLS/WS futures.
        for (route, stream_secs, tls_secs, ws_secs, succeeds, elapsed) in [
            (Route::Tor, 20, 2, 3, true, 25),
            (Route::Direct, 20, 0, 0, false, 15),
            (Route::Tor, 121, 0, 0, false, 120),
            (Route::Tor, 1, 8, 8, false, 16),
            (Route::Direct, 10, 3, 3, false, 15),
        ] {
            let connector = connector(policy(&["wss://example.com/"], RouteMode::Always).unwrap());
            let began = Instant::now();
            let result = connector
                .connection_phases(
                    route,
                    async {
                        let permit = connector.0.slots[0].clone().acquire_owned().await.unwrap();
                        tokio::time::sleep(Duration::from_secs(stream_secs)).await;
                        Ok(permit)
                    },
                    |permit| async move {
                        tokio::time::sleep(Duration::from_secs(tls_secs)).await;
                        tokio::time::sleep(Duration::from_secs(ws_secs)).await;
                        drop(permit);
                        Ok(())
                    },
                )
                .await;
            assert_eq!(result.is_ok(), succeeds);
            if !succeeds {
                assert!(matches!(result, Err(Error::Timeout)));
            }
            assert_eq!(Instant::now() - began, Duration::from_secs(elapsed));
            assert!(connector.0.slots[0].clone().try_acquire_owned().is_ok());
        }
    }

    #[tokio::test(start_paused = true)]
    async fn permit_wait_consumes_the_route_stream_budget() {
        for (route, budget) in [(Route::Direct, 15), (Route::Tor, 120)] {
            let connector = connector(policy(&["wss://example.com/"], RouteMode::Always).unwrap());
            let occupied = connector.0.slots[0].clone().acquire_owned().await.unwrap();
            let began = Instant::now();
            let result = connector
                .connection_phases(
                    route,
                    async { Ok(connector.0.slots[0].clone().acquire_owned().await.unwrap()) },
                    |_| async { Ok(()) },
                )
                .await;
            assert!(matches!(result, Err(Error::Timeout)));
            assert_eq!(Instant::now() - began, Duration::from_secs(budget));
            drop(occupied);
            assert!(connector.0.slots[0].clone().try_acquire_owned().is_ok());
        }
    }

    #[tokio::test(start_paused = true)]
    async fn cancellation_drops_permit_wait_stream_and_handshake() {
        for phase in 0..3 {
            let connector = connector(policy(&["wss://example.com/"], RouteMode::Always).unwrap());
            let occupied = if phase == 0 {
                Some(connector.0.slots[0].clone().acquire_owned().await.unwrap())
            } else {
                None
            };
            let cancel = connector.0.cancelled.clone();
            let canceller = tokio::spawn(async move {
                tokio::time::sleep(Duration::from_secs(2)).await;
                cancel.cancel();
            });
            let began = Instant::now();
            let result = connector
                .connection_phases(
                    Route::Tor,
                    async {
                        let permit = connector.0.slots[0].clone().acquire_owned().await.unwrap();
                        if phase == 1 {
                            std::future::pending::<()>().await;
                        }
                        Ok(permit)
                    },
                    |permit| async move {
                        let _permit = permit;
                        std::future::pending::<Result<()>>().await
                    },
                )
                .await;
            assert!(matches!(result, Err(Error::Transport("shutdown"))));
            assert_eq!(Instant::now() - began, Duration::from_secs(2));
            canceller.await.unwrap();
            drop(occupied);
            assert!(connector.0.slots[0].clone().try_acquire_owned().is_ok());
        }
    }

    #[tokio::test]
    async fn constructors_proxy_and_unconfigured_calls_never_connect() {
        let connector = connector(policy(&["wss://example.com/path"], RouteMode::Auto).unwrap());
        let _client = connector
            .sdk_client(Arc::new(nostr::key::Keys::generate()))
            .unwrap();
        assert!(!connector.0.ready.load(Ordering::Acquire));
        assert!(
            connector
                .open_socket(&Url::parse("wss://example.com/path").unwrap(), None)
                .await
                .is_err()
        );
        connector
            .bootstrap(Instant::now() + Duration::from_secs(1))
            .await
            .unwrap();
        assert!(
            connector
                .open_socket(
                    &Url::parse("wss://example.com/path").unwrap(),
                    Some("127.0.0.1:9050".parse().unwrap())
                )
                .await
                .is_err()
        );
        assert!(
            connector
                .open_socket(&Url::parse("wss://unconfigured.example").unwrap(), None)
                .await
                .is_err()
        );
        assert_eq!(connector.0.direct_attempts.load(Ordering::Relaxed), 0);
        connector
            .shutdown(Instant::now() + Duration::from_secs(1))
            .await
            .unwrap();
        assert!(
            connector
                .bootstrap(Instant::now() + Duration::from_secs(1))
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn tor_not_ready_never_uses_direct_connector() {
        let host = onion();
        let url = format!("ws://{host}/");
        let connector = connector(policy(&[&url], RouteMode::Auto).unwrap());
        // Model a failure after local readiness: no Arti client available.
        connector.0.ready.store(true, Ordering::Release);
        let result = connector
            .open_socket(&Url::parse(&url).unwrap(), None)
            .await;
        assert!(matches!(result, Err(Error::Transport("arti_not_ready"))));
        assert_eq!(connector.0.direct_attempts.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn sdk_mapping_is_byte_preserving() {
        let bytes = (0..=255).collect::<Vec<_>>();
        assert_eq!(
            from_native(NativeMessage::Binary(bytes.clone().into())).unwrap(),
            Message::Binary(bytes)
        );
        assert_eq!(
            from_native(NativeMessage::Text("a\0é".into())).unwrap(),
            Message::Text("a\0é".into())
        );
    }

    async fn memory_socket(
        max: usize,
    ) -> (
        RelayConnector,
        WebSocketSink,
        WebSocketStream,
        tokio_tungstenite::WebSocketStream<tokio::io::DuplexStream>,
    ) {
        let limits = TransportLimits {
            max_frame_bytes: max,
            ..TransportLimits::default()
        };
        let policy =
            RelayPolicy::parse(&["wss://example.com/".into()], RouteMode::Off, limits).unwrap();
        let connector = connector(policy);
        let permit = Arc::new(connector.0.slots[0].clone().acquire_owned().await.unwrap());
        let (client, server) = tokio::io::duplex(8192);
        let server =
            tokio::spawn(async move { tokio_tungstenite::accept_async(server).await.unwrap() });
        let (sink, stream) = connector
            .websocket(
                &Url::parse("wss://example.com/").unwrap(),
                Box::new(client),
                permit,
            )
            .await
            .unwrap();
        (connector, sink, stream, server.await.unwrap())
    }

    #[tokio::test]
    async fn websocket_enforces_frame_limit_before_sdk_decode() {
        let (_connector, _sink, mut stream, mut server) = memory_socket(1024).await;
        server
            .send(NativeMessage::Binary(vec![7; 2048].into()))
            .await
            .unwrap();
        assert!(
            timeout_at(Instant::now() + Duration::from_secs(1), stream.next())
                .await
                .unwrap()
                .unwrap()
                .is_err()
        );
    }

    #[tokio::test]
    async fn one_connection_permit_survives_either_half_and_shutdown_cancels_reader() {
        let (connector, sink, mut stream, _server) = memory_socket(1024).await;
        assert!(connector.0.slots[0].clone().try_acquire_owned().is_err());
        drop(sink);
        assert!(connector.0.slots[0].clone().try_acquire_owned().is_err());
        connector
            .shutdown(Instant::now() + Duration::from_secs(1))
            .await
            .unwrap();
        assert!(
            timeout_at(Instant::now() + Duration::from_secs(1), stream.next())
                .await
                .unwrap()
                .is_none()
        );
        drop(stream);
    }

    #[tokio::test]
    async fn outbound_oversize_rejected_and_binary_roundtrip_preserved() {
        let (_connector, mut sink, mut stream, mut server) = memory_socket(1024).await;
        let bytes = (0..=255).collect::<Vec<_>>();
        sink.send(Message::Binary(bytes.clone())).await.unwrap();
        assert_eq!(
            server.next().await.unwrap().unwrap(),
            NativeMessage::Binary(bytes.clone().into())
        );
        server
            .send(NativeMessage::Binary(bytes.clone().into()))
            .await
            .unwrap();
        assert_eq!(
            stream.next().await.unwrap().unwrap(),
            Message::Binary(bytes)
        );
        assert!(sink.send(Message::Binary(vec![0; 1025])).await.is_err());
    }
}
