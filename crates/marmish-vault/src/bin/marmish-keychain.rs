use base64::{engine::general_purpose::STANDARD, Engine as _};
use marmish_vault::{Error, KeyRef, Label, Metadata, Purpose, Secret, Vault};
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

#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "lowercase", deny_unknown_fields)]
enum Request {
    Probe {
        version: u8,
    },
    Create {
        version: u8,
        reference: Reference,
        label: String,
        secret: WireSecret,
    },
    Load {
        version: u8,
        reference: Reference,
    },
    List {
        version: u8,
    },
}
impl Request {
    fn version(&self) -> u8 {
        match self {
            Self::Probe { version }
            | Self::Create { version, .. }
            | Self::Load { version, .. }
            | Self::List { version } => *version,
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
        Request::Probe { .. } => {
            Vault::open_default()?.probe()?;
            Ok(Output::Empty {})
        }
        Request::Create {
            reference,
            label,
            secret,
            ..
        } => {
            let reference = reference.validated()?;
            let label = Label::new(&label)?;
            Vault::open_default()?.create(&reference, &label, &secret.0)?;
            Ok(Output::Empty {})
        }
        Request::Load { reference, .. } => {
            let reference = reference.validated()?;
            let secret = Vault::open_default()?.load(&reference)?;
            Ok(Output::Loaded {
                secret: WireSecret(secret),
            })
        }
        Request::List { .. } => {
            let items = Vault::open_default()?.list()?;
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
    let mut bytes =
        Zeroizing::new(serde_json::to_vec(&response).map_err(|_| Error::BackendFailure)?);
    bytes.push(b'\n');
    if bytes.len() > RESPONSE_LIMIT {
        return Err(Error::BackendFailure);
    }
    Ok(bytes)
}

fn harden() -> Result<(), Error> {
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
        .spawn(|| {
            std::thread::sleep(Duration::from_secs(15));
            std::process::exit(124);
        })
        .map_err(|_| Error::BackendFailure)?;
    Ok(())
}

fn main() -> ExitCode {
    let result = harden().and_then(|()| {
        if std::env::args_os().len() != 1 {
            return Err(Error::Malformed);
        }
        execute(parse(std::io::stdin().lock())?)
    });
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
