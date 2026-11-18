use std::{
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use futures_util::{SinkExt, StreamExt};
use nostr::{event::Event, key::PublicKey, types::Url};
use rcgen::{BasicConstraints, CertificateParams, IsCa, Issuer, KeyPair, KeyUsagePurpose};
use rustls::pki_types::PrivatePkcs8KeyDer;
use serde_json::{Value, json, value::RawValue};
use tokio::{
    net::{TcpListener, TcpStream},
    time::{Instant, timeout},
};
use tokio_rustls::{TlsAcceptor, server::TlsStream};
use tokio_tungstenite::{
    WebSocketStream,
    tungstenite::{Message, protocol::WebSocketConfig},
};

use crate::transport::{ArtiDirs, RelayConnector, RelayPolicy, RouteMode, TransportLimits};

const STEP: Duration = Duration::from_secs(5);
pub(super) const CHALLENGE: &str = "private peer proof";
pub(super) const FIXTURE_FRAME_LIMIT: usize = (1024 * 1024) + 1024;

pub(super) fn deadline() -> Instant {
    Instant::now() + STEP
}

pub(super) struct Relay {
    pub(super) url: Url,
    listener: TcpListener,
    tls: TlsAcceptor,
    roots: rustls::RootCertStore,
    next_challenge: AtomicUsize,
}

impl Relay {
    pub(super) async fn new() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = Url::parse(&format!("wss://{}/", listener.local_addr().unwrap())).unwrap();
        let (tls, roots) = tls();
        Self {
            url,
            listener,
            tls,
            roots,
            next_challenge: AtomicUsize::new(0),
        }
    }

    pub(super) fn connector(&self, path: &Path) -> RelayConnector {
        let policy = RelayPolicy::parse(
            &[self.url.to_string()],
            RouteMode::Off,
            TransportLimits::default(),
        )
        .unwrap();
        RelayConnector::for_loopback_fixture(
            policy,
            ArtiDirs {
                state: path.join("state"),
                cache: path.join("cache"),
            },
            self.url.clone(),
            self.roots.clone(),
        )
        .unwrap()
    }

    pub(super) async fn accept(&self) -> Peer {
        let (tcp, _) = timeout(STEP, self.listener.accept())
            .await
            .unwrap()
            .unwrap();
        tcp.set_nodelay(true).unwrap();
        let tls = timeout(STEP, self.tls.accept(tcp)).await.unwrap().unwrap();
        let config = WebSocketConfig::default()
            .write_buffer_size(16 * 1024)
            .max_message_size(Some(FIXTURE_FRAME_LIMIT))
            .max_frame_size(Some(FIXTURE_FRAME_LIMIT));
        let socket = timeout(
            STEP,
            tokio_tungstenite::accept_async_with_config(tls, Some(config)),
        )
        .await
        .unwrap()
        .unwrap();
        Peer { socket }
    }

    pub(super) async fn authenticated(&self, account: PublicKey) -> (Peer, Event) {
        let mut peer = self.accept().await;
        let n = self.next_challenge.fetch_add(1, Ordering::Relaxed);
        let challenge = format!("{CHALLENGE} {n}");
        peer.packet(json!(["AUTH", challenge])).await;
        let auth = peer.authentication(&self.url, account, &challenge).await;
        (peer, auth)
    }

    pub(super) async fn ready(&self, account: PublicKey) -> (Peer, String) {
        let (mut peer, auth) = self.authenticated(account).await;
        peer.ack(&auth, true).await;
        let sub = peer.request(account, None).await;
        peer.end(&sub).await;
        (peer, sub)
    }
}

pub(super) struct Peer {
    socket: WebSocketStream<TlsStream<TcpStream>>,
}

impl Peer {
    pub(super) async fn frame(&mut self, frame: Message) {
        timeout(STEP, self.socket.send(frame))
            .await
            .unwrap()
            .unwrap();
    }

    pub(super) async fn receive(&mut self) -> Message {
        timeout(STEP, self.socket.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap()
    }

    pub(super) async fn assert_no_request(&mut self) {
        if let Ok(Some(Ok(message))) = timeout(Duration::from_millis(80), self.socket.next()).await
        {
            assert!(matches!(message, Message::Close(_)), "{message:?}");
        }
    }

    pub(super) async fn packet(&mut self, value: Value) {
        self.frame(Message::Text(value.to_string().into())).await;
    }

    pub(super) async fn text(&mut self) -> String {
        let Message::Text(text) = self.receive().await else {
            panic!("expected a relay application message");
        };
        text.to_string()
    }

    pub(super) async fn authentication(
        &mut self,
        url: &Url,
        account: PublicKey,
        challenge: &str,
    ) -> Event {
        let text = self.text().await;
        let (verb, event): (String, Event) = serde_json::from_str(&text).unwrap();
        assert_eq!(verb, "AUTH");
        event.verify().unwrap();
        let fields = serde_json::to_value(&event).unwrap();
        assert_eq!(fields["kind"], 22242);
        assert_eq!(fields["pubkey"], account.to_hex());
        assert_eq!(fields["content"], "");
        assert_eq!(
            fields["tags"],
            json!([["relay", url.as_str()], ["challenge", challenge]])
        );
        event
    }

    pub(super) async fn request(&mut self, account: PublicKey, bounds: Option<[u64; 2]>) -> String {
        let text = self.text().await;
        let (verb, id, filter): (String, String, Value) = serde_json::from_str(&text).unwrap();
        assert_eq!(verb, "REQ");
        let components: Vec<_> = id.split('/').collect();
        let [prefix, namespace, tail @ ..] = components.as_slice() else {
            panic!("bad subscription ID");
        };
        assert_eq!(*prefix, "mp");
        assert!(id.len() <= 64);
        assert!(namespace.len() == 32 && hex::decode(namespace).is_ok());
        let recipient = account.to_hex();
        let expected = match bounds {
            Some([since, until]) => {
                let ["p", seq] = tail else {
                    panic!("expected historical request")
                };
                assert!(seq.parse::<u64>().is_ok_and(|value| value != 0));
                json!({"kinds": [7435], "#p": [recipient], "since": since, "until": until, "limit": 4})
            }
            None => {
                assert_eq!(tail, ["l"]);
                json!({"kinds": [7435], "#p": [recipient], "limit": 0})
            }
        };
        assert_eq!(filter, expected);
        id
    }

    pub(super) async fn ack(&mut self, event: &Event, accepted: bool) {
        self.packet(json!(["OK", event.id, accepted, "fixture detail"]))
            .await;
    }

    pub(super) async fn publication(&mut self, original: &[u8], account: PublicKey) -> Event {
        let text = self.text().await;
        let (verb, raw): (String, &RawValue) = serde_json::from_str(&text).unwrap();
        assert_eq!(verb, "EVENT");
        assert_eq!(raw.get().as_bytes(), original);
        let event: Event = serde_json::from_str(raw.get()).unwrap();
        assert_ne!(event.pubkey, account);
        assert_eq!(u16::from(event.kind), 7435);
        event.verify().unwrap();
        event
    }

    pub(super) async fn deliver(&mut self, sub: &str, raw: &str) {
        self.frame(event_message(sub, raw)).await;
    }

    pub(super) async fn batch(&mut self, frames: Vec<Message>) {
        timeout(STEP, async {
            for frame in frames {
                self.socket.feed(frame).await?;
            }
            self.socket.flush().await
        })
        .await
        .unwrap()
        .unwrap();
    }

    pub(super) async fn end(&mut self, sub: &str) {
        self.packet(json!(["EOSE", sub])).await;
    }

    pub(super) async fn close(&mut self, sub: &str) {
        let close: Value = serde_json::from_str(&self.text().await).unwrap();
        assert_eq!(close, json!(["CLOSE", sub]));
    }
}

pub(super) fn event_message(sub: &str, raw: &str) -> Message {
    let sub = serde_json::to_string(sub).unwrap();
    Message::Text(format!("[\"EVENT\",{sub},{raw}]").into())
}

fn tls() -> (TlsAcceptor, rustls::RootCertStore) {
    let mut ca = CertificateParams::default();
    ca.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    ca.key_usages.push(KeyUsagePurpose::KeyCertSign);
    let ca_key = KeyPair::generate().unwrap();
    let ca_cert = ca.self_signed(&ca_key).unwrap();
    let issuer = Issuer::new(ca, ca_key);
    let leaf_key = KeyPair::generate().unwrap();
    let leaf = CertificateParams::new(vec!["127.0.0.1".into()])
        .unwrap()
        .signed_by(&leaf_key, &issuer)
        .unwrap();
    let config = rustls::ServerConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .unwrap()
    .with_no_client_auth()
    .with_single_cert(
        vec![leaf.der().clone()],
        PrivatePkcs8KeyDer::from(leaf_key.serialize_der()).into(),
    )
    .unwrap();
    let mut roots = rustls::RootCertStore::empty();
    roots.add(ca_cert.der().clone()).unwrap();
    (TlsAcceptor::from(Arc::new(config)), roots)
}
