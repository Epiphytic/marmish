use crate::{Error, KeyRef, Label, Metadata, Namespace, Purpose, Secret, Vault};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::io::{Read, Write};
use std::process::ExitCode;
use std::time::Duration;
use zeroize::Zeroizing;

const REQUEST_LIMIT: usize = 8192;
const RESPONSE_LIMIT: usize = 131072;
const LIST_LIMIT: usize = 128;

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
enum WirePurpose {
    Nostr,
    Sqlcipher,
}

/// UI policy is selected only by the dedicated hidden helper entrypoint.
#[derive(Clone, Copy)]
pub enum AuthorizationMode {
    Interactive,
    Verify,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum AuthorizationCategory {
    #[cfg(any(not(target_os = "macos"), test))]
    Unsupported,
    Malformed,
    BackendFailure,
    #[cfg(any(target_os = "macos", test))]
    InteractionNotAllowed,
    #[cfg(any(target_os = "macos", test))]
    AuthenticationFailed,
    #[cfg(any(target_os = "macos", test))]
    UserCancelled,
    #[cfg(any(target_os = "macos", test))]
    Missing,
    #[cfg(any(target_os = "macos", test))]
    Uninitialized,
    #[cfg(any(target_os = "macos", test))]
    KeychainLocked,
    #[cfg(any(target_os = "macos", test))]
    Ambiguous,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub(super) enum AuthorizationPurpose {
    Provider,
    #[cfg(any(target_os = "macos", test))]
    Nostr,
    #[cfg(any(target_os = "macos", test))]
    Sqlcipher,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct AuthorizationFailure {
    pub(super) category: AuthorizationCategory,
    pub(super) purpose: AuthorizationPurpose,
}

impl AuthorizationFailure {
    fn provider(category: AuthorizationCategory) -> Self {
        Self {
            category,
            purpose: AuthorizationPurpose::Provider,
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "kebab-case")]
enum AuthorizationNamespace {
    MarmishV2,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AuthorizationRequest {
    version: u8,
    namespace: AuthorizationNamespace,
    nostr: Reference,
    sqlcipher: Reference,
}

fn parse_authorization(reader: impl Read) -> Result<[KeyRef; 2], AuthorizationFailure> {
    let malformed = || AuthorizationFailure::provider(AuthorizationCategory::Malformed);
    let mut bytes = Zeroizing::new(Vec::with_capacity(REQUEST_LIMIT + 1));
    reader
        .take((REQUEST_LIMIT + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| malformed())?;
    if bytes.len() > REQUEST_LIMIT {
        return Err(malformed());
    }
    let request: AuthorizationRequest = serde_json::from_slice(&bytes).map_err(|_| malformed())?;
    let AuthorizationNamespace::MarmishV2 = request.namespace;
    if request.version != 1 {
        return Err(malformed());
    }
    let nostr = request.nostr.validated().map_err(|_| malformed())?;
    let sqlcipher = request.sqlcipher.validated().map_err(|_| malformed())?;
    if nostr.version() != 2
        || sqlcipher.version() != 2
        || nostr.purpose() != Purpose::Nostr
        || sqlcipher.purpose() != Purpose::Sqlcipher
    {
        return Err(malformed());
    }
    Ok([nostr, sqlcipher])
}

#[derive(Serialize)]
#[serde(untagged)]
enum AuthorizationResponse {
    Success {
        version: u8,
        ok: bool,
        result: AuthorizationEmpty,
    },
    Failure {
        version: u8,
        ok: bool,
        error: AuthorizationCategory,
        purpose: AuthorizationPurpose,
    },
}

#[derive(Serialize)]
struct AuthorizationEmpty {}

fn encode_authorization(
    result: Result<(), AuthorizationFailure>,
) -> Result<Zeroizing<Vec<u8>>, Error> {
    let response = match result {
        Ok(()) => AuthorizationResponse::Success {
            version: 1,
            ok: true,
            result: AuthorizationEmpty {},
        },
        Err(failure) => AuthorizationResponse::Failure {
            version: 1,
            ok: false,
            error: failure.category,
            purpose: failure.purpose,
        },
    };
    serialize_response(&response, REQUEST_LIMIT)
}

fn execute_authorization(
    references: [KeyRef; 2],
    mode: AuthorizationMode,
) -> Result<(), AuthorizationFailure> {
    #[cfg(target_os = "macos")]
    {
        crate::macos::authorize_existing(&references, mode)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (references, mode);
        Err(AuthorizationFailure::provider(
            AuthorizationCategory::Unsupported,
        ))
    }
}

/// Authorize/read only the two validated existing v2 references. Never emits secrets.
pub fn run_authorization(mode: AuthorizationMode) -> ExitCode {
    let watchdog = match mode {
        AuthorizationMode::Interactive => Duration::from_secs(120),
        AuthorizationMode::Verify => Duration::from_secs(15),
    };
    let result = harden_with_watchdog(watchdog)
        .map_err(|_| AuthorizationFailure::provider(AuthorizationCategory::BackendFailure))
        .and_then(|()| parse_authorization(std::io::stdin().lock()))
        .and_then(|references| execute_authorization(references, mode));
    let success = result.is_ok();
    match encode_authorization(result) {
        Ok(bytes) if std::io::stdout().lock().write_all(&bytes).is_ok() && success => {
            ExitCode::SUCCESS
        }
        _ => ExitCode::FAILURE,
    }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Reference {
    version: u8,
    purpose: WirePurpose,
    id: String,
}
impl Reference {
    fn validated(self) -> Result<KeyRef, Error> {
        let purpose = match self.purpose {
            WirePurpose::Nostr => Purpose::Nostr,
            WirePurpose::Sqlcipher => Purpose::Sqlcipher,
        };
        KeyRef::new(self.version, purpose, &self.id)
    }
}
impl From<&KeyRef> for Reference {
    fn from(reference: &KeyRef) -> Self {
        Self {
            version: reference.version(),
            purpose: match reference.purpose() {
                Purpose::Nostr => WirePurpose::Nostr,
                Purpose::Sqlcipher => WirePurpose::Sqlcipher,
            },
            id: reference.id().to_owned(),
        }
    }
}

struct WireSecret(Secret);
impl<'de> Deserialize<'de> for WireSecret {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let encoded = Zeroizing::new(String::deserialize(deserializer)?);
        let decoded = Zeroizing::new(
            STANDARD
                .decode(encoded.as_bytes())
                .map_err(|_| serde::de::Error::custom("malformed"))?,
        );
        let canonical = Zeroizing::new(STANDARD.encode(&decoded));
        if canonical.as_str() != encoded.as_str() {
            return Err(serde::de::Error::custom("malformed"));
        }
        Secret::from_slice(&decoded)
            .map(Self)
            .map_err(|_| serde::de::Error::custom("malformed"))
    }
}
impl Serialize for WireSecret {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let encoded = Zeroizing::new(STANDARD.encode(self.0.as_bytes()));
        serializer.serialize_str(&encoded)
    }
}

#[derive(Clone, Copy, Default, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum WireNamespace {
    #[default]
    Legacy,
    MarmishV2,
}
impl WireNamespace {
    fn native(self) -> Namespace {
        match self {
            Self::Legacy => Namespace::Legacy,
            Self::MarmishV2 => Namespace::MarmishV2,
        }
    }
}

#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "lowercase", deny_unknown_fields)]
enum Request {
    Probe {
        version: u8,
        #[serde(default)]
        namespace: WireNamespace,
    },
    #[serde(rename = "derive_nostr")]
    DeriveNostr { version: u8, secret: WireSecret },
    Create {
        version: u8,
        #[serde(default)]
        namespace: WireNamespace,
        reference: Reference,
        label: String,
        secret: WireSecret,
    },
    Load {
        version: u8,
        #[serde(default)]
        namespace: WireNamespace,
        reference: Reference,
    },
    List {
        version: u8,
        #[serde(default)]
        namespace: WireNamespace,
    },
}
impl Request {
    fn version(&self) -> u8 {
        match self {
            Self::Probe { version, .. }
            | Self::DeriveNostr { version, .. }
            | Self::Create { version, .. }
            | Self::Load { version, .. }
            | Self::List { version, .. } => *version,
        }
    }
}

#[derive(Serialize)]
struct PublicMetadata {
    reference: Reference,
    label: String,
}
impl From<Metadata> for PublicMetadata {
    fn from(metadata: Metadata) -> Self {
        Self {
            reference: Reference::from(&metadata.key_ref),
            label: metadata.label.as_str().to_owned(),
        }
    }
}

#[derive(Serialize)]
#[serde(untagged)]
enum Output {
    Empty {},
    Derived { public_key: String },
    Loaded { secret: WireSecret },
    Listed { items: Vec<PublicMetadata> },
}
#[derive(Serialize)]
#[serde(untagged)]
enum Response {
    Success {
        version: u8,
        ok: bool,
        result: Output,
    },
    Failure {
        version: u8,
        ok: bool,
        error: &'static str,
    },
}

fn parse(reader: impl Read) -> Result<Request, Error> {
    let mut bytes = Zeroizing::new(Vec::with_capacity(REQUEST_LIMIT + 1));
    reader
        .take((REQUEST_LIMIT + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| Error::Malformed)?;
    if bytes.len() > REQUEST_LIMIT {
        return Err(Error::Malformed);
    }
    let request: Request = serde_json::from_slice(&bytes).map_err(|_| Error::Malformed)?;
    if request.version() != 1 {
        return Err(Error::Malformed);
    }
    Ok(request)
}

fn execute(request: Request) -> Result<Output, Error> {
    // Validate before connecting: malformed input must never contact a vault.
    match request {
        Request::DeriveNostr { secret, .. } => {
            let mut scalar = secp256k1::SecretKey::from_slice(secret.0.as_bytes())
                .map_err(|_| Error::Malformed)?;
            let mut keypair =
                secp256k1::Keypair::from_secret_key(&secp256k1::Secp256k1::new(), &scalar);
            let (public, _) = keypair.x_only_public_key();
            scalar.non_secure_erase();
            keypair.non_secure_erase();
            Ok(Output::Derived {
                public_key: public.to_string(),
            })
        }
        Request::Probe { namespace, .. } => {
            Vault::open_namespaced(namespace.native())?.probe()?;
            Ok(Output::Empty {})
        }
        Request::Create {
            reference,
            namespace,
            label,
            secret,
            ..
        } => {
            let reference = reference.validated()?;
            if reference.version() != namespace.native().version() {
                return Err(Error::Malformed);
            }
            let label = Label::new(&label)?;
            Vault::open_namespaced(namespace.native())?.create(&reference, &label, &secret.0)?;
            Ok(Output::Empty {})
        }
        Request::Load {
            reference,
            namespace,
            ..
        } => {
            let reference = reference.validated()?;
            if reference.version() != namespace.native().version() {
                return Err(Error::Malformed);
            }
            let secret = Vault::open_namespaced(namespace.native())?.load(&reference)?;
            Ok(Output::Loaded {
                secret: WireSecret(secret),
            })
        }
        Request::List { namespace, .. } => {
            let items = Vault::open_namespaced(namespace.native())?.list()?;
            if items.len() > LIST_LIMIT {
                return Err(Error::BackendFailure);
            }
            Ok(Output::Listed {
                items: items.into_iter().map(PublicMetadata::from).collect(),
            })
        }
    }
}

fn encode(result: Result<Output, Error>) -> Result<Zeroizing<Vec<u8>>, Error> {
    let response = match result {
        Ok(result) => Response::Success {
            version: 1,
            ok: true,
            result,
        },
        Err(error) => Response::Failure {
            version: 1,
            ok: false,
            error: error.category(),
        },
    };
    serialize_response(&response, RESPONSE_LIMIT)
}

fn serialize_response(response: &impl Serialize, cap: usize) -> Result<Zeroizing<Vec<u8>>, Error> {
    let mut bytes =
        Zeroizing::new(serde_json::to_vec(response).map_err(|_| Error::BackendFailure)?);
    bytes.push(b'\n');
    if bytes.len() > cap {
        return Err(Error::BackendFailure);
    }
    Ok(bytes)
}

fn harden() -> Result<(), Error> {
    harden_with_watchdog(Duration::from_secs(15))
}

fn harden_with_watchdog(watchdog: Duration) -> Result<(), Error> {
    std::panic::set_hook(Box::new(|_| {
        eprintln!("marmish-keychain: backend_failure");
    }));
    let limit = libc::rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };
    if unsafe { libc::setrlimit(libc::RLIMIT_CORE, &limit) } != 0 {
        return Err(Error::BackendFailure);
    }
    std::thread::Builder::new()
        .name("keychain-watchdog".into())
        .spawn(move || {
            std::thread::sleep(watchdog);
            std::process::exit(124);
        })
        .map_err(|_| Error::BackendFailure)?;
    Ok(())
}

pub fn run() -> ExitCode {
    let result = harden().and_then(|()| execute(parse(std::io::stdin().lock())?));
    let success = result.is_ok();
    let (bytes, success) = match encode(result) {
        Ok(bytes) => (Ok(bytes), success),
        Err(_) => (encode(Err(Error::BackendFailure)), false),
    };
    match bytes {
        Ok(bytes) if std::io::stdout().lock().write_all(&bytes).is_ok() && success => {
            ExitCode::SUCCESS
        }
        Ok(_) | Err(_) => ExitCode::FAILURE,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn authorization_fixture() -> String {
        format!(
            r#"{{"version":1,"namespace":"marmish-v2","nostr":{{"version":2,"purpose":"nostr","id":"{}"}},"sqlcipher":{{"version":2,"purpose":"sqlcipher","id":"{}"}}}}"#,
            "a".repeat(64),
            "b".repeat(64)
        )
    }

    #[test]
    fn authorization_request_is_closed_and_exactly_two_v2_purposes() {
        let input = authorization_fixture();
        let parsed = parse_authorization(input.as_bytes()).unwrap();
        assert_eq!(
            parsed[0],
            KeyRef::new(2, Purpose::Nostr, &"a".repeat(64)).unwrap()
        );
        assert_eq!(
            parsed[1],
            KeyRef::new(2, Purpose::Sqlcipher, &"b".repeat(64)).unwrap()
        );
        let malformed = AuthorizationFailure::provider(AuthorizationCategory::Malformed);
        for invalid in [
            input.replacen("\"version\":1", "\"version\":2", 1),
            input.replacen("\"version\":1", "\"version\":1,\"version\":1", 1),
            input.replace("marmish-v2", "legacy"),
            input.replacen("\"version\":2", "\"version\":1", 1),
            input.replacen("\"purpose\":\"nostr\"", "\"purpose\":\"sqlcipher\"", 1),
            input.replacen("\"purpose\":\"sqlcipher\"", "\"purpose\":\"nostr\"", 1),
            input.replacen(
                "\"purpose\":\"nostr\"",
                "\"purpose\":\"nostr\",\"purpose\":\"nostr\"",
                1,
            ),
            input.replacen("\"id\":", "\"extra\":true,\"id\":", 1),
            input.replacen(
                "\"version\":1",
                "\"version\":1,\"secret\":\"private-marker\"",
                1,
            ),
            input.replacen("\"version\":1", "\"version\":1,\"interactive\":true", 1),
            input.replacen("\"version\":1", "\"version\":1,\"action\":\"create\"", 1),
            input.replace(&"a".repeat(64), &"A".repeat(64)),
            input.replace(&"a".repeat(64), &"a".repeat(63)),
            format!("{input}{input}"),
            format!("{input}x"),
            "{}".into(),
            "x".repeat(REQUEST_LIMIT + 1),
        ] {
            assert_eq!(parse_authorization(invalid.as_bytes()), Err(malformed));
        }
        // Ordinary load protocol cannot enable interactive authorization.
        assert!(parse(input.as_bytes()).is_err());
        assert!(parse(br#"{"version":1,"action":"authorize"}"#.as_slice()).is_err());
    }

    #[test]
    fn response_cap_includes_the_required_newline() {
        assert_eq!(serialize_response(&"x", 4).unwrap().as_slice(), b"\"x\"\n");
        assert!(matches!(
            serialize_response(&"x", 3),
            Err(Error::BackendFailure)
        ));
    }

    #[test]
    fn authorization_responses_never_include_secret_or_reference_data() {
        assert_eq!(
            encode_authorization(Ok(())).unwrap().as_slice(),
            b"{\"version\":1,\"ok\":true,\"result\":{}}\n"
        );
        let cases = [
            (AuthorizationCategory::Unsupported, "unsupported"),
            (AuthorizationCategory::Malformed, "malformed"),
            (AuthorizationCategory::BackendFailure, "backend_failure"),
            (
                AuthorizationCategory::InteractionNotAllowed,
                "interaction_not_allowed",
            ),
            (
                AuthorizationCategory::AuthenticationFailed,
                "authentication_failed",
            ),
            (AuthorizationCategory::UserCancelled, "user_cancelled"),
            (AuthorizationCategory::Missing, "missing"),
            (AuthorizationCategory::Uninitialized, "uninitialized"),
            (AuthorizationCategory::KeychainLocked, "keychain_locked"),
            (AuthorizationCategory::Ambiguous, "ambiguous"),
        ];
        for (category, literal) in cases {
            for (purpose, purpose_literal) in [
                (AuthorizationPurpose::Provider, "provider"),
                (AuthorizationPurpose::Nostr, "nostr"),
                (AuthorizationPurpose::Sqlcipher, "sqlcipher"),
            ] {
                let bytes =
                    encode_authorization(Err(AuthorizationFailure { category, purpose })).unwrap();
                let expected = format!(
                    "{{\"version\":1,\"ok\":false,\"error\":\"{literal}\",\"purpose\":\"{purpose_literal}\"}}\n"
                );
                assert_eq!(bytes.as_slice(), expected.as_bytes());
                assert!(bytes.len() <= REQUEST_LIMIT);
            }
        }
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn authorization_on_other_platforms_never_opens_a_provider() {
        for mode in [AuthorizationMode::Interactive, AuthorizationMode::Verify] {
            let refs = parse_authorization(authorization_fixture().as_bytes()).unwrap();
            assert_eq!(
                execute_authorization(refs, mode),
                Err(AuthorizationFailure::provider(
                    AuthorizationCategory::Unsupported
                ))
            );
        }
    }

    #[test]
    fn derive_nostr_is_stateless_and_validates_scalars() {
        let mut bytes = [0u8; 32];
        bytes[31] = 1;
        let output = execute(Request::DeriveNostr {
            version: 1,
            secret: WireSecret(Secret::from_bytes(bytes)),
        })
        .unwrap();
        match output {
            Output::Derived { public_key } => assert_eq!(
                public_key,
                "79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798"
            ),
            _ => panic!("incorrect result"),
        }
        assert!(matches!(
            execute(Request::DeriveNostr {
                version: 1,
                secret: WireSecret(Secret::from_bytes([0; 32])),
            }),
            Err(Error::Malformed)
        ));
        assert!(matches!(
            execute(Request::DeriveNostr {
                version: 1,
                secret: WireSecret(Secret::from_bytes([255; 32])),
            }),
            Err(Error::Malformed)
        ));
    }

    #[test]
    fn strict_protocol_rejects_unknown_duplicate_trailing_and_versions() {
        for input in [
            r#"{"action":"probe","version":1,"extra":true}"#,
            r#"{"action":"probe","version":1,"version":1}"#,
            r#"{"action":"probe","action":"probe","version":1}"#,
            r#"{"action":"probe","version":2}"#,
            r#"{"action":"probe","version":1} {}"#,
            r#"{"action":"probe"}"#,
            r#"{"action":"other","version":1}"#,
        ] {
            assert!(matches!(parse(input.as_bytes()), Err(Error::Malformed)));
        }
        assert!(parse(br#"{"action":"probe","version":1}"#.as_slice()).is_ok());
        assert!(matches!(
            parse(vec![b' '; REQUEST_LIMIT + 1].as_slice()),
            Err(Error::Malformed)
        ));
    }

    #[test]
    fn namespaces_default_legacy_and_reject_cross_version_before_provider() {
        for (namespace, version) in [("marmish-v2", 1), ("legacy", 2)] {
            let raw = format!(
                r#"{{"action":"load","version":1,"namespace":"{namespace}","reference":{{"version":{version},"purpose":"nostr","id":"{}"}}}}"#,
                "a".repeat(64)
            );
            assert!(matches!(
                execute(parse(raw.as_bytes()).unwrap()),
                Err(Error::Malformed)
            ));
        }
        assert!(
            parse(br#"{"action":"probe","version":1,"namespace":"marmish-v2"}"#.as_slice()).is_ok()
        );
        assert!(
            parse(br#"{"action":"probe","version":1,"namespace":"unknown"}"#.as_slice()).is_err()
        );
    }

    #[test]
    fn references_and_secrets_are_strict_and_errors_contain_no_input() {
        let reference = format!(
            r#"{{"version":1,"purpose":"nostr","id":"{}"}}"#,
            "a".repeat(64)
        );
        let valid = STANDARD.encode([0u8; 32]);
        for secret in [
            valid.trim_end_matches('=').to_owned(),
            "private diagnostic marker".into(),
            STANDARD.encode([0u8; 31]),
        ] {
            let input = format!(
                r#"{{"version":1,"action":"create","reference":{reference},"label":"test","secret":"{secret}"}}"#
            );
            assert!(matches!(parse(input.as_bytes()), Err(Error::Malformed)));
        }
        let input = format!(
            r#"{{"version":1,"action":"load","reference":{{"version":1,"purpose":"nostr","id":"{}","extra":1}}}}"#,
            "a".repeat(64)
        );
        assert!(matches!(parse(input.as_bytes()), Err(Error::Malformed)));
        let output = encode(Err(Error::Malformed)).unwrap();
        assert_eq!(
            output.as_slice(),
            b"{\"version\":1,\"ok\":false,\"error\":\"malformed\"}\n"
        );
    }

    #[test]
    fn response_encoding_is_bounded_and_round_trips_secret_bytes() {
        let bytes = encode(Ok(Output::Loaded {
            secret: WireSecret(Secret::from_bytes([255; 32])),
        }))
        .unwrap();
        let output: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(
            STANDARD
                .decode(output["result"]["secret"].as_str().unwrap())
                .unwrap(),
            [255; 32]
        );
        let too_large = Output::Listed {
            items: vec![PublicMetadata {
                reference: Reference {
                    version: 1,
                    purpose: WirePurpose::Nostr,
                    id: "a".repeat(64),
                },
                label: "x".repeat(RESPONSE_LIMIT),
            }],
        };
        assert!(matches!(encode(Ok(too_large)), Err(Error::BackendFailure)));
    }
}
