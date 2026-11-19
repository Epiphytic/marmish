use super::*;
use serde_json::json;
use tokio::{io::AsyncReadExt, sync::oneshot};

fn selection() -> Selection {
    Selection {
        server: nostr::key::Keys::generate().public_key(),
        offer_id: [0xab; 16],
        revision: [0xcd; 32],
        generation: 1,
    }
}

fn decode_response(selection: Selection, response: Response) -> Result<(), Failure> {
    let wire = serde_json::to_vec(&response).unwrap();
    let envelope = serde_json::from_slice(&wire).map_err(|_| Failure::Uncertain)?;
    decode(selection, envelope)
}

#[test]
fn duplicate_or_explicit_sidecar_fields_are_refused_before_decoding() {
    let selector = selection();
    let wire = serde_json::to_string(&response(selector, Ok(()))).unwrap();
    let malformed = [
        wire.replace(r#""version":1"#, r#""version":2,"version":1"#),
        wire.replace(r#""generation":1"#, r#""generation":2,"generation":1"#),
        wire.replacen('{', r#"{"command":null,"#, 1),
    ];
    for body in malformed {
        assert_ne!(body, wire);
        assert!(serde_json::from_str::<Envelope>(&body).is_err());
    }
}

#[test]
fn approval_request_and_recorded_reply_use_exact_closed_shapes() {
    let selector = selection();
    let request = LocalRequest::PeerApprove {
        request: Request::new(selector),
    };
    assert!(!request.is_command());
    assert!(request.uses_short_exchange());
    let raw = serde_json::to_value(&request).unwrap();
    assert!(serde_json::from_value::<LocalRequest>(raw.clone()).is_ok());
    for (section, field, value) in [
        ("request", "extra", json!(true)),
        (
            "selection",
            "server",
            json!(
                raw["request"]["selection"]["server"]
                    .as_str()
                    .unwrap()
                    .to_ascii_uppercase()
            ),
        ),
        ("selection", "revision", json!("CD".repeat(32))),
        ("selection", "generation", json!(0)),
    ] {
        let mut malformed = raw.clone();
        let target = if section == "request" {
            &mut malformed["request"]
        } else {
            &mut malformed["request"]["selection"]
        };
        target[field] = value;
        assert!(serde_json::from_value::<LocalRequest>(malformed).is_err());
    }
    let mut future = raw;
    future["request"]["version"] = json!(2);
    let LocalRequest::PeerApprove { request } = serde_json::from_value(future).unwrap() else {
        panic!("approval request")
    };
    assert!(!request.is_supported());

    let recorded = || response(selector, Ok(()));
    assert_eq!(decode_response(selector, recorded()), Ok(()));
    let stale = Selection {
        generation: 2,
        ..selector
    };
    assert_eq!(decode_response(stale, recorded()), Err(Failure::Uncertain));
    for (field, value) in [
        ("version", json!(2)),
        ("state", json!("pending")),
        ("expires_at", json!(8)),
    ] {
        let mut invalid = recorded();
        invalid.result.as_mut().unwrap()[field] = value;
        assert_eq!(decode_response(selector, invalid), Err(Failure::Uncertain));
    }
    let mut mixed = recorded();
    mixed.command = Some(crate::command_local::LocalResult::LocalBusy);
    assert_eq!(decode_response(selector, mixed), Err(Failure::Uncertain));
    let mut mixed_failure = response(selector, Err(Failure::Busy));
    mixed_failure.result = Some(json!({"untrusted":true}));
    assert_eq!(
        decode_response(selector, mixed_failure),
        Err(Failure::Uncertain)
    );
    for failure in [
        Failure::Busy,
        Failure::Unavailable,
        Failure::Expired,
        Failure::Stale,
        Failure::InvalidRequest,
        Failure::Uncertain,
    ] {
        assert_eq!(
            decode_response(selector, response(selector, Err(failure))),
            Err(failure)
        );
    }
}

#[tokio::test(start_paused = true)]
async fn approval_client_stays_open_and_its_deadline_can_precede_the_listener() {
    let directory = tempfile::tempdir().unwrap();
    use std::os::unix::fs::PermissionsExt as _;
    std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let path = directory.path().join("s");
    let listener = super::super::bind(&path).unwrap();
    let selector = selection();
    let deadline = Instant::now() + IO_TIMEOUT;
    let (wrote, written) = oneshot::channel();
    let task = tokio::spawn(async move {
        written.await.unwrap();
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        let (mut stream, _) = listener.accept().await.unwrap();
        let server_deadline = Instant::now() + IO_TIMEOUT;
        let LocalRequest::PeerApprove { request } = read_frame(&mut stream).await.unwrap() else {
            panic!("approval request")
        };
        assert_eq!(request.selection, selector);
        assert!(
            matches!(stream.try_read(&mut [0; 1]), Err(error) if error.kind() == std::io::ErrorKind::WouldBlock)
        );
        assert_eq!(stream.read(&mut [0; 1]).await.unwrap(), 0);
        assert!(Instant::now() < server_deadline);
    });
    wrote.send(()).unwrap();
    assert_eq!(
        client_request(&path, selector).await,
        Err(Failure::Uncertain)
    );
    assert_eq!(Instant::now(), deadline);
    task.await.unwrap();
}
