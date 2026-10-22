use marmish_core::{ConfigError, FrameError, Limits, ProbeEnvelope, RelaySet, Retention};

fn relays(values: &[&str], loopback: bool) -> Result<RelaySet, ConfigError> {
    RelaySet::parse(
        &values.iter().map(|v| (*v).to_owned()).collect::<Vec<_>>(),
        loopback,
    )
}

#[test]
fn canonical_urls_deduplicate_in_order() {
    let set = relays(
        &[
            "WSS://EXAMPLE.COM:443",
            "wss://example.com/",
            "wss://example.com./",
            "ws://example.org:80",
            "wss://bücher.example/",
            "wss://xn--bcher-kva.example",
            "wss://[2606:4700:4700:0000:0000:0000:0000:1111]:443/",
            "wss://[2606:4700:4700::1111]/",
        ],
        false,
    )
    .unwrap();
    assert_eq!(
        set.as_slice(),
        [
            "wss://example.com/",
            "ws://example.org/",
            "wss://xn--bcher-kva.example/",
            "wss://[2606:4700:4700::1111]/"
        ]
    );
}

#[test]
fn no_defaults_or_extra_url_components() {
    assert!(matches!(
        relays(&[], false),
        Err(ConfigError::EmptyRelaySet)
    ));
    for value in [
        "",
        "https://example.com",
        "wss://user:secret@example.com",
        "wss://@example.com",
        "wss://example.com?",
        "wss://example.com#",
        "wss://example.com/path",
        "wss://a.example,wss://b.example",
        "wss://a%2cb.example",
        " wss://example.com",
        "wss://example.com\n",
        "wss://example.com\\",
        "wss://bad..example",
        "wss://-bad.example",
    ] {
        assert!(relays(&[value], false).is_err(), "accepted {value:?}");
    }
}

#[test]
fn loopback_is_explicit_and_private_literals_stay_denied() {
    for host in [
        "localhost",
        "LOCALHOST.",
        "test.localhost",
        "127.0.0.1",
        "127.1",
        "[::1]",
    ] {
        let value = format!("ws://{host}");
        assert!(matches!(
            relays(&[&value], false),
            Err(ConfigError::LoopbackDenied)
        ));
        assert!(relays(&[&value], true).is_ok(), "rejected {host}");
    }
    for host in [
        "0.0.0.0",
        "10.1.2.3",
        "172.16.0.1",
        "192.168.1.1",
        "169.254.1.1",
        "100.64.1.1",
        "192.0.2.1",
        "198.18.0.1",
        "224.0.0.1",
        "255.255.255.255",
        "[::]",
        "[fc00::1]",
        "[fe80::1]",
        "[ff02::1]",
        "[::ffff:127.0.0.1]",
        "[2001:db8::1]",
        "[2002:7f00:1::]",
    ] {
        assert!(
            relays(&[&format!("ws://{host}")], true).is_err(),
            "accepted {host}"
        );
    }
}

#[test]
fn diagnostics_never_echo_raw_values() {
    let error = relays(
        &["wss://sensitive-user:sensitive-password@example.com"],
        false,
    )
    .err()
    .unwrap();
    assert!(!format!("{error} {error:?}").contains("sensitive"));
}

#[test]
fn retention_distinguishes_default_from_disabled_and_bounds_numbers() {
    assert_eq!(Retention::default().seconds(), Some(86_400));
    assert_eq!(Retention::from_seconds(None).unwrap().seconds(), None);
    for seconds in [1, Retention::MAX_SECONDS] {
        assert_eq!(
            Retention::from_seconds(Some(seconds)).unwrap().seconds(),
            Some(seconds)
        );
    }
    for seconds in [0, Retention::MAX_SECONDS + 1, u64::MAX] {
        assert_eq!(
            Retention::from_seconds(Some(seconds)),
            Err(ConfigError::RetentionOutOfRange)
        );
    }
}

#[test]
fn bounded_opaque_bytes_round_trip() {
    for bytes in [
        vec![],
        vec![0, 255, 128, b'\n'],
        vec![255; Limits::RAW_BYTES],
    ] {
        let frame = ProbeEnvelope::encode(&bytes).unwrap();
        assert!(frame.len() <= Limits::JSON_BYTES);
        assert_eq!(ProbeEnvelope::decode(&frame).unwrap(), bytes);
    }
    assert_eq!(
        ProbeEnvelope::encode(&vec![0; Limits::RAW_BYTES + 1]),
        Err(FrameError::SizeLimit)
    );
    assert_eq!(
        ProbeEnvelope::decode(&vec![b' '; Limits::JSON_BYTES + 1]),
        Err(FrameError::SizeLimit)
    );
}

#[test]
fn hostile_json_version_and_encoding_are_rejected() {
    for frame in [
        r#"{}"#,
        r#"{"version":1,"payload":4}"#,
        r#"{"version":1,"payload":"","execute":"anything"}"#,
        r#"{"version":1,"version":1,"payload":""}"#,
        r#"{"version":1,"payload":""} trailing"#,
    ] {
        assert_eq!(
            ProbeEnvelope::decode(frame.as_bytes()),
            Err(FrameError::InvalidJson)
        );
    }
    assert_eq!(
        ProbeEnvelope::decode(br#"{"version":2,"payload":""}"#),
        Err(FrameError::InvalidVersion)
    );
    for payload in ["Zg", "Zg=", "Zh==", "Zg===", "Zg==\n", "____", "é"] {
        let frame = serde_json::json!({"version":1,"payload":payload}).to_string();
        assert_eq!(
            ProbeEnvelope::decode(frame.as_bytes()),
            Err(FrameError::InvalidEncoding)
        );
    }
    // The maximum encoded length can encode two more bytes than the raw cap.
    let frame =
        serde_json::json!({"version":1,"payload":"A".repeat(Limits::RAW_BYTES.div_ceil(3)*4)})
            .to_string();
    assert_eq!(
        ProbeEnvelope::decode(frame.as_bytes()),
        Err(FrameError::SizeLimit)
    );
    let frame = serde_json::json!({"version":1,"payload":"A".repeat(30_000)}).to_string();
    assert_eq!(
        ProbeEnvelope::decode(frame.as_bytes()),
        Err(FrameError::SizeLimit)
    );
}
