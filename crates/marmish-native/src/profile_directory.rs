//! Operator-initiated NIP-05 lookup. This returns a directory candidate;
//! signed metadata verification and approval belong to the caller.

use crate::command_wire::Account;
use crate::transport::{NetworkStream, RelayConnector, RouteMode, tls_stream};
use crate::{Error, Result};
use hickory_resolver::TokioResolver;
use http_body_util::{BodyExt, Empty};
use hyper::{
    Request, StatusCode,
    body::{Body, Bytes},
    client::conn::http1,
    header,
};
use hyper_util::rt::TokioIo;
use nostr::key::PublicKey;
use serde::Serialize;
use serde::de::{self, Deserialize, Deserializer, MapAccess, Visitor};
use serde_json::value::RawValue;
use std::collections::{BTreeMap, BTreeSet};
use std::net::{IpAddr, SocketAddr};
use std::pin::Pin;
use std::str::FromStr;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::task::{Context, Poll, ready};
use std::time::Duration;
use std::{fmt, io};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::{net::TcpStream, time::timeout};
use tokio_util::sync::CancellationToken;

const LOOKUP_TIMEOUT: Duration = Duration::from_secs(15);
const DNS_TIMEOUT: Duration = Duration::from_secs(5);
const TCP_TIMEOUT: Duration = Duration::from_secs(2);
const HEADER_LIMIT: usize = 64 * 1024;
const BODY_LIMIT: usize = 64 * 1024;
const ADDRESS_LIMIT: usize = 16;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DirectoryId {
    name: String,
    domain: String,
}

impl FromStr for DirectoryId {
    type Err = Error;

    fn from_str(value: &str) -> Result<Self> {
        let invalid = Error::Invalid("nip05_identifier");
        let (name, domain) = value.split_once('@').ok_or(invalid)?;
        if name.is_empty()
            || name.len() > 64
            || !name
                .bytes()
                .all(|c| matches!(c, b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.'))
            || !valid_domain(domain)
        {
            return Err(Error::Invalid("nip05_identifier"));
        }
        let domain = domain.to_ascii_lowercase();
        if domain.ends_with(".onion") {
            return Err(Error::Invalid("nip05_onion_directory"));
        }
        Ok(Self {
            name: name.into(),
            domain,
        })
    }
}

impl fmt::Display for DirectoryId {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(out, "{}@{}", self.name, self.domain)
    }
}

fn valid_domain(domain: &str) -> bool {
    if domain.len() > 253 || !domain.contains('.') {
        return false;
    }
    let labels_valid = domain.split('.').all(|label| {
        let bytes = label.as_bytes();
        !bytes.is_empty()
            && bytes.len() <= 63
            && bytes[0].is_ascii_alphanumeric()
            && bytes[bytes.len() - 1].is_ascii_alphanumeric()
            && bytes
                .iter()
                .all(|c| c.is_ascii_alphanumeric() || *c == b'-')
    });
    // Reject alternative numeric IPv4 spellings as well as plain IP literals.
    labels_valid && matches!(url::Host::parse(domain), Ok(url::Host::Domain(_)))
}

pub(crate) fn validate_email(value: &str) -> Result<()> {
    let (local, domain) = value
        .split_once('@')
        .ok_or(Error::Invalid("public_email"))?;
    if value.len() > 254 || local.len() > 64 || !valid_domain(domain) {
        return Err(Error::Invalid("public_email"));
    }
    if !local.split('.').all(|atom| {
        !atom.is_empty()
            && atom
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"!#$%&'*+-/=?^_`{|}~".contains(&c))
    }) {
        return Err(Error::Invalid("public_email"));
    }
    Ok(())
}

pub(crate) async fn resolve(connector: &RelayConnector, id: &DirectoryId) -> Result<Account> {
    timeout(LOOKUP_TIMEOUT, async {
        let stream = match connector.policy().mode() {
            RouteMode::Always => connector.directory_tor_stream(&id.domain).await?,
            RouteMode::Auto | RouteMode::Off => direct_stream(&id.domain).await?,
        };
        let stream = tls_stream(stream, &id.domain).await?;
        let body = https_get(stream, id).await?;
        directory_key(&body, &id.name)
    })
    .await
    .map_err(|_| Error::Transport("directory_https_timeout"))?
}

async fn direct_stream(host: &str) -> Result<Box<dyn NetworkStream>> {
    let resolver = TokioResolver::builder_tokio()
        .map_err(|_| Error::Transport("directory_dns_configuration"))?
        .build()
        .map_err(|_| Error::Transport("directory_dns_configuration"))?;
    // Absolute DNS name prevents resolver search suffixes.
    let answers = timeout(DNS_TIMEOUT, resolver.lookup_ip(format!("{host}.")))
        .await
        .map_err(|_| Error::Transport("directory_dns_timeout"))?
        .map_err(|_| Error::Transport("directory_dns"))?;
    let mut seen = BTreeSet::new();
    let addresses: Vec<SocketAddr> = answers
        .iter()
        .filter(|ip| public_ip(*ip) && seen.insert(*ip))
        .take(ADDRESS_LIMIT)
        .map(|ip| SocketAddr::new(ip, 443))
        .collect();
    if addresses.is_empty() {
        return Err(Error::Transport("directory_nonpublic_destination"));
    }
    for address in addresses {
        // Connect the exact address that passed validation. No second DNS lookup.
        if let Ok(Ok(stream)) = timeout(TCP_TIMEOUT, TcpStream::connect(address)).await {
            return Ok(Box::new(stream));
        }
    }
    Err(Error::Transport("directory_tcp"))
}

// Reject the IANA special-purpose blocks, even entries marked globally
// reachable, plus multicast. IPv6 is restricted to public unicast space.
fn public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => !matches!(
            ip.octets(),
            [0 | 10 | 127 | 224..=255, ..]
                | [100, 64..=127, ..]
                | [169, 254, ..]
                | [172, 16..=31, ..]
                | [192, 0, 0 | 2, _]
                | [192, 31, 196, _]
                | [192, 52, 193, _]
                | [192, 88, 99, _]
                | [192, 168, ..]
                | [192, 175, 48, _]
                | [198, 18 | 19, ..]
                | [198, 51, 100, _]
                | [203, 0, 113, _]
        ),
        IpAddr::V6(ip) => {
            let segments = ip.segments();
            segments[0] & 0xe000 == 0x2000
                && !matches!(
                    segments,
                    [0x2001, 0..=0x01ff | 0x0db8, ..]
                        | [0x2002, ..]
                        | [0x2620, 0x004f, 0x8000, ..]
                        | [0x3fff, 0..=0x0fff, ..]
                )
        }
    }
}

// Hyper's adaptive max buffer is checked while a head is incomplete. A
// complete oversized head can arrive in the last read. Bound the wire head
// independently without interpreting status or header fields here.
struct HeaderBoundedStream {
    stream: Box<dyn NetworkStream>,
    head_bytes: usize,
    tail: u32,
    head_complete: Arc<AtomicBool>,
}

impl AsyncRead for HeaderBoundedStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let offset = buf.filled().len();
        ready!(Pin::new(&mut self.stream).poll_read(cx, buf))?;
        if !self.head_complete.load(Ordering::Relaxed) {
            for &byte in &buf.filled()[offset..] {
                self.head_bytes += 1;
                // Hyper tolerates leading blank lines. Require the status line
                // first so the byte limit includes the whole response head.
                if b"HTTP/"
                    .get(self.head_bytes - 1)
                    .is_some_and(|&expected| byte != expected)
                {
                    return Poll::Ready(Err(io::Error::other(
                        "invalid directory HTTP status line",
                    )));
                }
                if self.head_bytes > HEADER_LIMIT {
                    return Poll::Ready(Err(io::Error::other("directory HTTP head exceeds limit")));
                }
                self.tail = (self.tail << 8) | u32::from(byte);
                if self.tail == u32::from_be_bytes(*b"\r\n\r\n") {
                    self.head_complete.store(true, Ordering::Relaxed);
                    break;
                }
            }
        }
        Poll::Ready(Ok(()))
    }
}

impl AsyncWrite for HeaderBoundedStream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.stream).poll_write(cx, bytes)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.stream).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.stream).poll_shutdown(cx)
    }
}

async fn https_get(stream: Box<dyn NetworkStream>, id: &DirectoryId) -> Result<Vec<u8>> {
    let query = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("name", &id.name)
        .finish();
    let mut request = Request::get(format!("/.well-known/nostr.json?{query}"))
        .header(header::HOST, &id.domain)
        .header(header::ACCEPT, "application/json")
        .header(header::ACCEPT_ENCODING, "identity")
        .header(header::CONNECTION, "close")
        .body(Empty::<Bytes>::new())
        .map_err(|_| Error::Transport("directory_http_request"))?;
    let informational = CancellationToken::new();
    let signal = informational.clone();
    hyper::ext::on_informational(&mut request, move |_| signal.cancel());
    let head_complete = Arc::new(AtomicBool::new(false));
    let bounded = HeaderBoundedStream {
        stream,
        head_bytes: 0,
        tail: 0,
        head_complete: head_complete.clone(),
    };
    let (mut sender, connection) = http1::Builder::new()
        .max_buf_size(HEADER_LIMIT)
        .max_headers(64)
        .handshake(TokioIo::new(bounded))
        .await
        .map_err(|_| Error::Transport("directory_http_handshake"))?;
    let exchange = async {
        let response = sender
            .send_request(request)
            .await
            .map_err(|_| Error::Transport("directory_http_response"))?;
        if !head_complete.load(Ordering::Relaxed) {
            return Err(Error::Transport("directory_http_response"));
        }
        if response.status() != StatusCode::OK {
            return Err(Error::Transport("directory_http_status"));
        }
        let mut body = response.into_body();
        if body
            .size_hint()
            .upper()
            .is_some_and(|n| n > BODY_LIMIT as u64)
        {
            return Err(Error::Transport("directory_http_body_size"));
        }
        let mut bytes = Vec::new();
        while let Some(frame) = body.frame().await {
            let data = frame
                .map_err(|_| Error::Transport("directory_http_body"))?
                .into_data()
                .map_err(|_| Error::Transport("directory_http_trailers"))?;
            if data.len() > BODY_LIMIT - bytes.len() {
                return Err(Error::Transport("directory_http_body_size"));
            }
            bytes.extend_from_slice(&data);
        }
        Ok(bytes)
    };
    tokio::pin!(exchange);
    // Drive the connection in this future. Completion, error or the outer
    // deadline drops both halves, with no detached socket task.
    let result = tokio::select! {
        biased;
        _ = informational.cancelled() => return Err(Error::Transport("directory_http_status")),
        result = &mut exchange => result,
        driver = connection => {
            driver.map_err(|_| Error::Transport("directory_http_connection"))?;
            exchange.await
        }
    };
    if informational.is_cancelled() {
        return Err(Error::Transport("directory_http_status"));
    }
    result
}

#[derive(Serialize)]
#[serde(transparent)]
pub(crate) struct UniqueFields<T>(pub(crate) BTreeMap<String, T>);

impl<'de, T: Deserialize<'de>> Deserialize<'de> for UniqueFields<T> {
    fn deserialize<D: Deserializer<'de>>(de: D) -> std::result::Result<Self, D::Error> {
        struct Object<T>(std::marker::PhantomData<T>);
        impl<'de, T: Deserialize<'de>> Visitor<'de> for Object<T> {
            type Value = UniqueFields<T>;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("an object with unique fields")
            }

            fn visit_map<M: MapAccess<'de>>(
                self,
                mut map: M,
            ) -> std::result::Result<Self::Value, M::Error> {
                let mut fields = BTreeMap::new();
                while let Some((name, value)) = map.next_entry()? {
                    if fields.insert(name, value).is_some() {
                        return Err(de::Error::custom("duplicate object field"));
                    }
                }
                Ok(UniqueFields(fields))
            }
        }
        de.deserialize_map(Object(std::marker::PhantomData))
    }
}

fn directory_key(body: &[u8], name: &str) -> Result<Account> {
    let malformed = || Error::Transport("directory_json");
    let UniqueFields::<&RawValue>(root) = serde_json::from_slice(body).map_err(|_| malformed())?;
    let names = root.get("names").ok_or_else(malformed)?.get();
    let UniqueFields::<&RawValue>(names) = serde_json::from_str(names).map_err(|_| malformed())?;
    let value = names
        .get(name)
        .ok_or(Error::Transport("directory_name_missing"))?
        .get();
    let value: String =
        serde_json::from_str(value).map_err(|_| Error::Transport("directory_invalid_key"))?;
    if value.len() != 64
        || !value
            .bytes()
            .all(|c| matches!(c, b'0'..=b'9' | b'a'..=b'f'))
    {
        return Err(Error::Transport("directory_invalid_key"));
    }
    let key = PublicKey::from_hex(&value).map_err(|_| Error::Transport("directory_invalid_key"))?;
    key.xonly()
        .map_err(|_| Error::Transport("directory_invalid_key"))?;
    Ok(*key.as_bytes())
}

#[cfg(test)]
const TEST_KEY: &str = "79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798";

#[cfg(test)]
mod core_tests {
    use super::*;
    use crate::transport::{ArtiDirs, RelayPolicy, TransportLimits};

    #[test]
    fn identifier_is_strict_and_domain_is_canonical() {
        assert_eq!(
            "a-b_1.c@Nostr.Example"
                .parse::<DirectoryId>()
                .unwrap()
                .to_string(),
            "a-b_1.c@nostr.example"
        );
        for input in [
            "Bob@nostr.example",
            "bob+tag@nostr.example",
            "bob?name@nostr.example",
            "bob@example",
            "@nostr.example",
            "bob@nostr..example",
            "bob@nostr.example.",
            "bob@-nostr.example",
            "bob@nostr-.example",
            "bob@nostr😃.example",
            "bob@127.0.0.1",
            "bob@0x7f.0x1",
            "bob@[::1]",
            "bob@nostr.example:443",
            "bob@nostr.example/",
            "bob@nostr.example@evil.example",
            "bob@nostr.example\n",
        ] {
            assert!(
                matches!(
                    input.parse::<DirectoryId>(),
                    Err(Error::Invalid("nip05_identifier"))
                ),
                "{input:?}"
            );
        }
        assert!(matches!(
            "bob@fake.ONION".parse::<DirectoryId>(),
            Err(Error::Invalid("nip05_onion_directory"))
        ));
        assert!(
            format!("{}@nostr.example", "x".repeat(64))
                .parse::<DirectoryId>()
                .is_ok()
        );
        assert!(
            format!("{}@nostr.example", "x".repeat(65))
                .parse::<DirectoryId>()
                .is_err()
        );
    }

    #[test]
    fn public_email_requires_ascii_dot_atoms_and_dns_name() {
        assert!(validate_email("A+b.x!#$%&'*-+/=?^_`{|}~@Nostr.Example").is_ok());
        for input in [
            "",
            "a..b@nostr.example",
            ".a@nostr.example",
            "a.@nostr.example",
            "a@nostr.example,b@other.example",
            "<a@nostr.example>",
            "Alice <a@nostr.example>",
            "\"a\"@nostr.example",
            "a@127.0.0.1",
            "é@example.com",
            "a@nostr.example\r\n",
        ] {
            assert!(validate_email(input).is_err(), "{input:?}");
        }
        let labels = [
            "x".repeat(63),
            "x".repeat(63),
            "x".repeat(63),
            "x".repeat(60),
        ];
        let domain = labels.join(".");
        assert_eq!(domain.len(), 252);
        assert!(validate_email(&format!("x@{domain}")).is_ok());
        assert!(validate_email(&format!("xx@{domain}")).is_err());
        assert!(validate_email(&format!("{}@nostr.example", "x".repeat(65))).is_err());
    }

    #[test]
    fn only_exact_unique_name_and_valid_lower_hex_is_used() {
        let document = format!(
            r#"{{"names":{{"bob":"{TEST_KEY}","other":null}},"relays":{{"ignored":["ws://127.0.0.1"]}}}}"#
        );
        let expected = PublicKey::from_hex(TEST_KEY).unwrap();
        assert_eq!(
            directory_key(document.as_bytes(), "bob").unwrap(),
            *expected.as_bytes()
        );
        assert!(matches!(
            directory_key(document.as_bytes(), "bo"),
            Err(Error::Transport("directory_name_missing"))
        ));
        for malformed in [
            r#"{"names":{"bob":"a","bob":"b"}}"#,
            r#"{"names":{"bob":"a","\u0062ob":"a"}}"#,
            r#"{"names":{},"names":{}}"#,
            r#"{"names":{},"relays":{},"relays":{}}"#,
            r#"{"names":[]}"#,
            r#"{}"#,
        ] {
            assert!(
                matches!(
                    directory_key(malformed.as_bytes(), "bob"),
                    Err(Error::Transport("directory_json"))
                ),
                "{malformed}"
            );
        }
        for invalid in [
            TEST_KEY.to_ascii_uppercase(),
            "00".repeat(32),
            "ff".repeat(32),
            "1234".to_owned(),
        ] {
            let malformed = format!(r#"{{"names":{{"bob":"{invalid}"}}}}"#);
            assert!(matches!(
                directory_key(malformed.as_bytes(), "bob"),
                Err(Error::Transport("directory_invalid_key"))
            ));
        }
    }

    #[test]
    fn excludes_special_addresses_and_v4_in_v6() {
        for ip in [
            "0.0.0.0",
            "10.2.3.4",
            "100.127.255.255",
            "127.0.1.1",
            "169.254.169.254",
            "172.31.255.255",
            "192.0.0.9",
            "192.31.196.1",
            "192.88.99.2",
            "192.168.1.1",
            "192.0.2.1",
            "198.19.30.1",
            "198.51.100.1",
            "203.0.113.1",
            "224.0.0.1",
            "239.1.2.3",
            "240.0.0.1",
            "255.255.255.255",
            "::",
            "::1",
            "::ffff:8.8.8.8",
            "::8.8.8.8",
            "64:ff9b::a00:0001",
            "fc00::1",
            "fe80::1",
            "ff02::1",
            "2001:db8::1",
            "2001:1::3",
            "2002:7f00:1::1",
            "2620:4f:8000::1",
            "3fff:fff::1",
        ] {
            assert!(!public_ip(ip.parse().unwrap()), "{ip}");
        }
        for ip in [
            "1.1.1.1",
            "100.128.0.1",
            "172.32.0.1",
            "2001:4860::1",
            "2606:4700::1111",
        ] {
            assert!(public_ip(ip.parse().unwrap()), "{ip}");
        }
    }

    #[tokio::test]
    async fn forced_tor_refuses_an_unbootstrapped_connector() {
        let root = tempfile::tempdir().unwrap();
        let relay = RelayConnector::new(
            RelayPolicy::parse(
                &["wss://nostr.example/".to_owned()],
                RouteMode::Always,
                TransportLimits::default(),
            )
            .unwrap(),
            ArtiDirs {
                state: root.path().join("state"),
                cache: root.path().join("cache"),
            },
        )
        .unwrap();
        let id: DirectoryId = "bob@localhost.example".parse().unwrap();
        assert!(matches!(
            resolve(&relay, &id).await,
            Err(Error::Transport("directory_tor_not_ready"))
        ));
    }
}

#[cfg(test)]
mod https_tests {
    use super::*;
    use rcgen::{BasicConstraints, CertificateParams, IsCa, Issuer, KeyPair, KeyUsagePurpose};
    use rustls::pki_types::PrivatePkcs8KeyDer;
    use std::sync::Arc;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;
    use tokio::time::Instant;
    use tokio_rustls::TlsAcceptor;

    const HOST: &str = "nostr.example";
    const LOCAL_BUDGET: Duration = Duration::from_secs(5);

    struct Fixture {
        listener: TcpListener,
        acceptor: TlsAcceptor,
        roots: rustls::RootCertStore,
    }

    impl Fixture {
        async fn new() -> Self {
            let mut ca = CertificateParams::default();
            ca.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
            ca.key_usages.push(KeyUsagePurpose::KeyCertSign);
            let ca_key = KeyPair::generate().unwrap();
            let ca_cert = ca.self_signed(&ca_key).unwrap();
            let issuer = Issuer::new(ca, ca_key);

            let cert = CertificateParams::new(vec![HOST.into()]).unwrap();
            let key = KeyPair::generate().unwrap();
            let cert = cert.signed_by(&key, &issuer).unwrap();
            let config = rustls::ServerConfig::builder_with_provider(Arc::new(
                rustls::crypto::ring::default_provider(),
            ))
            .with_safe_default_protocol_versions()
            .unwrap()
            .with_no_client_auth()
            .with_single_cert(
                vec![cert.der().clone()],
                PrivatePkcs8KeyDer::from(key.serialize_der()).into(),
            )
            .unwrap();
            let mut roots = rustls::RootCertStore::empty();
            roots.add(ca_cert.der().clone()).unwrap();
            Self {
                listener: TcpListener::bind("127.0.0.1:0").await.unwrap(),
                acceptor: TlsAcceptor::from(Arc::new(config)),
                roots,
            }
        }

        async fn serve(&self, raw_response: &[u8]) {
            let (raw, _) = self.listener.accept().await.unwrap();
            let Ok(mut stream) = self.acceptor.accept(raw).await else {
                return;
            };
            let mut request = Vec::new();
            while !request.ends_with(b"\r\n\r\n") {
                assert!(request.len() < 2048);
                request.push(stream.read_u8().await.unwrap());
            }
            let request = std::str::from_utf8(&request).unwrap();
            assert!(request.starts_with("GET /.well-known/nostr.json?name=bob HTTP/1.1\r\n"));
            assert!(request.contains("\r\nhost: nostr.example\r\n"));
            assert!(request.contains("\r\nconnection: close\r\n"));
            // Status/size rejection may close the stream before its body.
            let _ = stream.write_all(raw_response).await;
            let _ = stream.shutdown().await;
        }

        async fn lookup(
            &self,
            response: &[u8],
            host: &str,
            roots: rustls::RootCertStore,
        ) -> Result<Account> {
            let id: DirectoryId = format!("bob@{host}").parse().unwrap();
            let lookup = async {
                let tcp = TcpStream::connect(self.listener.local_addr().unwrap()).await?;
                let tls =
                    crate::transport::tls_stream_with_roots(Box::new(tcp), host, roots).await?;
                let body = https_get(tls, &id).await?;
                directory_key(&body, &id.name)
            };
            let (result, ()) = timeout(LOCAL_BUDGET, async {
                tokio::join!(lookup, self.serve(response))
            })
            .await
            .map_err(|_| Error::Transport("directory_fixture_timeout"))?;
            result
        }
    }

    fn valid_response() -> Vec<u8> {
        let body = format!(r#"{{"names":{{"bob":"{TEST_KEY}"}}}}"#);
        format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
        .into_bytes()
    }

    #[tokio::test]
    #[ignore = "uses a bounded real loopback TLS fixture"]
    async fn verifies_certificate_and_bounds_http() {
        let fixture = Fixture::new().await;
        let success = valid_response();
        let account = fixture
            .lookup(&success, HOST, fixture.roots.clone())
            .await
            .unwrap();
        assert_eq!(hex::encode(account), TEST_KEY);
        assert!(matches!(
            fixture
                .lookup(&success, HOST, rustls::RootCertStore::empty())
                .await,
            Err(Error::Transport("tls_verification"))
        ));
        assert!(matches!(
            fixture
                .lookup(&success, "wrong.example", fixture.roots.clone())
                .await,
            Err(Error::Transport("tls_verification"))
        ));
        let redirect = b"HTTP/1.1 302 Found\r\nLocation: https://nostr.example/forward\r\nContent-Length: 0\r\n\r\n";
        assert!(matches!(
            fixture.lookup(redirect, HOST, fixture.roots.clone()).await,
            Err(Error::Transport("directory_http_status"))
        ));
        let informational = [
            b"HTTP/1.1 103 Early Hints\r\nLink: </other>\r\n\r\n".as_slice(),
            &success,
        ]
        .concat();
        assert!(matches!(
            fixture
                .lookup(&informational, HOST, fixture.roots.clone())
                .await,
            Err(Error::Transport("directory_http_status"))
        ));
        let declared_large = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n",
            BODY_LIMIT + 1
        );
        assert!(matches!(
            fixture
                .lookup(declared_large.as_bytes(), HOST, fixture.roots.clone())
                .await,
            Err(Error::Transport("directory_http_body_size"))
        ));
        let chunked_large = format!(
            "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n{:x}\r\n{}\r\n0\r\n\r\n",
            BODY_LIMIT + 1,
            "a".repeat(BODY_LIMIT + 1)
        );
        assert!(matches!(
            fixture
                .lookup(chunked_large.as_bytes(), HOST, fixture.roots.clone())
                .await,
            Err(Error::Transport("directory_http_body_size"))
        ));
        let prefix = "HTTP/1.1 200 OK\r\nX-Test: ";
        let body = format!(r#"{{"names":{{"bob":"{TEST_KEY}"}}}}"#);
        let ending = format!(
            "\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        let exact = "a".repeat(HEADER_LIMIT - prefix.len() - ending.len());
        let exact_header = format!("{prefix}{exact}{ending}{body}");
        assert_eq!(
            hex::encode(
                fixture
                    .lookup(exact_header.as_bytes(), HOST, fixture.roots.clone())
                    .await
                    .unwrap()
            ),
            TEST_KEY
        );
        let wide_header = format!("{prefix}{exact}a{ending}{body}");
        assert!(matches!(
            fixture
                .lookup(wide_header.as_bytes(), HOST, fixture.roots.clone())
                .await,
            Err(Error::Transport(
                "directory_http_response" | "directory_http_connection"
            ))
        ));
        let preamble = [b"\r\n\r\n".as_slice(), exact_header.as_bytes()].concat();
        assert!(matches!(
            fixture.lookup(&preamble, HOST, fixture.roots.clone()).await,
            Err(Error::Transport(
                "directory_http_response" | "directory_http_connection"
            ))
        ));
    }

    #[tokio::test]
    #[ignore = "opt-in loopback TLS latency benchmark"]
    async fn loopback_lookup_latency() {
        let fixture = Fixture::new().await;
        let response = valid_response();
        for _ in 0..5 {
            fixture
                .lookup(&response, HOST, fixture.roots.clone())
                .await
                .unwrap();
        }
        let mut samples = Vec::new();
        let mut failed = 0;
        for _ in 0..100 {
            let began = Instant::now();
            match fixture.lookup(&response, HOST, fixture.roots.clone()).await {
                Ok(key) if hex::encode(key) == TEST_KEY => {
                    samples.push(began.elapsed().as_micros())
                }
                _ => failed += 1,
            }
        }
        samples.sort_unstable();
        if samples.is_empty() {
            panic!("all {failed} loopback HTTPS samples failed");
        }
        let percentile = |p: usize| samples[(samples.len() * p).div_ceil(100) - 1];
        eprintln!(
            "HTTPS loopback NIP-05 (TCP+TLS+HTTP+JSON, no DNS): n={} failed={failed} p50_us={} p95_us={} p99_us={}",
            samples.len(),
            percentile(50),
            percentile(95),
            percentile(99),
        );
        assert_eq!(failed, 0);
    }
}
