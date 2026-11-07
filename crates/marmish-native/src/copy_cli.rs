//! Literal regular-file copy frontend; disconnect and cancellation are distinct.
use crate::{
    Error, Result,
    command_cli::Context,
    copy_io::CopyIo,
    copy_local_gc::{Disposition, Lease},
    copy_worker::{Cancellation, Worker},
    stream_cli::ExecutionSignals,
    stream_session::{ControlHandle, StreamSession},
};
use marmish_stream::{
    codec::Codec,
    copy_protocol::{self as protocol, AttemptMode, Direction, Launch},
    wire::{Binding, RequestBody, ResponseBody, Terminal},
};
use std::{
    ffi::OsString,
    os::unix::ffi::OsStringExt,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

pub(crate) struct Options {
    pub direction: Direction,
    pub source: OsString,
    pub destination: OsString,
    pub resume: Option<String>,
    pub overwrite: bool,
    pub preserve_mode: bool,
    pub preserve_mtime: bool,
    pub compress: bool,
    pub idle: Duration,
    pub keepalive: Duration,
    pub probe: Duration,
    pub retention: u64,
    pub new_session: bool,
    pub signals: ExecutionSignals,
}
fn invalid(reason: &'static str) -> Error {
    Error::Invalid(reason)
}
fn diagnostic(args: std::fmt::Arguments<'_>) {
    use std::io::Write;
    let _ = writeln!(std::io::stderr().lock(), "{args}");
}
fn transfer_id(value: Option<&str>) -> Result<([u8; 32], AttemptMode)> {
    match value {
        None => {
            let mut id = [0; 32];
            getrandom::fill(&mut id).map_err(|_| Error::Identity("random"))?;
            Ok((id, AttemptMode::Create))
        }
        Some(text) => {
            if text.len() != 64
                || text
                    .bytes()
                    .any(|b| !b.is_ascii_digit() && !(b'a'..=b'f').contains(&b))
            {
                return Err(invalid("copy_transfer_id"));
            }
            let mut id = [0; 32];
            hex::decode_to_slice(text, &mut id).map_err(|_| invalid("copy_transfer_id"))?;
            Ok((id, AttemptMode::Resume))
        }
    }
}
fn admission_failure(code: marmish_stream::wire::CopyAdmissionError) -> Error {
    match code {
        marmish_stream::wire::CopyAdmissionError::RecoveryRequired => invalid("recovery_required"),
        marmish_stream::wire::CopyAdmissionError::Unsupported => {
            Error::Copy(protocol::CopyError::Unsupported)
        }
        marmish_stream::wire::CopyAdmissionError::Denied => {
            Error::Stream(marmish_stream::wire::WireErrorCode::Denied)
        }
        marmish_stream::wire::CopyAdmissionError::Busy => Error::Copy(protocol::CopyError::Busy),
        marmish_stream::wire::CopyAdmissionError::Conflict => {
            Error::Copy(protocol::CopyError::Conflict)
        }
        marmish_stream::wire::CopyAdmissionError::Capacity => {
            Error::Copy(protocol::CopyError::Quota)
        }
    }
}
async fn open(
    session: &mut StreamSession,
    launch: &Launch,
    codec: Codec,
    probe: Duration,
) -> Result<()> {
    match session
        .setup_with_timeout(
            RequestBody::CopyHello {
                versions: vec![protocol::VERSION],
                codecs: vec![Codec::None, Codec::Zlib],
            },
            probe,
        )
        .await
        .map_err(|error| {
            if matches!(error, Error::Timeout) {
                invalid("copy_capability_unavailable")
            } else {
                error
            }
        })? {
        ResponseBody::CopyHello {
            version,
            codecs,
            frame_bytes,
            window_bytes,
        } if version == protocol::VERSION
            && codecs.len() == 2
            && codecs.contains(&Codec::None)
            && codecs.contains(&Codec::Zlib)
            && codecs.contains(&codec)
            && frame_bytes as usize == marmish_stream::MAX_BLOCK
            && window_bytes as usize == marmish_stream::WINDOW_BYTES => {}
        _ => return Err(invalid("copy_capability_unavailable")),
    }
    match session
        .setup(RequestBody::OpenCopy {
            launch: launch.clone(),
            codec,
        })
        .await?
    {
        ResponseBody::Opened {
            codec: agreed,
            open_sha256,
            disposition: marmish_stream::wire::OpenDisposition::Created,
        } if agreed == codec
            && open_sha256 == launch.digest(codec).map_err(|_| invalid("copy_launch"))? =>
        {
            Ok(())
        }
        ResponseBody::CopyRefused {
            code,
            prior_binding,
        } => {
            diagnostic(format_args!("marmish copy: admission {code:?}"));
            if let Some(prior) = prior_binding {
                diagnostic(format_args!(
                    "marmish copy: prior stream {}; inspect or cancel it before retrying resume",
                    hex::encode(prior.stream)
                ));
            }
            Err(admission_failure(code))
        }
        ResponseBody::Error { code } => {
            diagnostic(format_args!("marmish copy: remote {code:?}"));
            Err(Error::Stream(code))
        }
        _ => Err(invalid("copy_open_response")),
    }
}
async fn cancel(
    control: &ControlHandle,
    signals: &mut ExecutionSignals,
    local: &AtomicBool,
) -> Result<u8> {
    local.store(true, Ordering::Release);
    if !control.open_attempted() {
        return Err(invalid("stream_interrupted_before_open"));
    }
    diagnostic(format_args!(
        "marmish copy: requesting remote cancellation; an already committed destination remains committed"
    ));
    tokio::select! {
        biased;
        signal=signals.interrupted()=>{signal?;Err(invalid("stream_cancellation_uncertain"))},
        result=control.cancel()=>match result? {
            Terminal::Cancelled|Terminal::Exited {..}|Terminal::Signaled {..}=>{diagnostic(format_args!("marmish copy: remote terminal cleanup confirmed"));Ok(130)},
            Terminal::Failed {..}=>Err(invalid("stream_cancellation_uncertain")),
        }
    }
}
pub(crate) async fn execute(
    context: Context,
    remote: [u8; 32],
    relays: Vec<String>,
    options: Options,
) -> Result<u8> {
    let _diagnostics = crate::stream_cli::Nonblocking::descriptors(&[2])?;
    let Options {
        direction,
        source,
        destination,
        resume,
        overwrite,
        preserve_mode,
        preserve_mtime,
        compress,
        idle,
        keepalive,
        probe,
        retention,
        new_session,
        mut signals,
    } = options;
    if !(protocol::MIN_RETENTION_SECS as u64..=protocol::MAX_RETENTION_SECS).contains(&retention) {
        return Err(invalid("copy_retention"));
    }
    if probe.is_zero() {
        return Err(invalid("copy_probe_timeout"));
    }
    let probe = probe.min(idle);
    if keepalive.is_zero() || keepalive >= idle {
        return Err(invalid("copy_keepalive"));
    }
    let (transfer, mode) = transfer_id(resume.as_deref())?;
    let source = source.into_vec();
    let destination = destination.into_vec();
    // Reject malformed paths before local daemon or remote room work. Full
    // no-follow traversal belongs to the filesystem worker at each endpoint.
    if [&source, &destination]
        .iter()
        .any(|p| p.is_empty() || p.len() > protocol::MAX_PATH || p.contains(&0))
    {
        return Err(invalid("copy_path"));
    }
    let (session, client) = tokio::select! {
        biased;
        signal=signals.interrupted()=>{signal?;return Err(invalid("stream_interrupted_before_open"));},
        result=context.prepare(remote,relays.clone(),new_session)=>result?,
    };
    let mut stream = [0; 16];
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| invalid("stream_clock"))?
        .as_secs();
    stream[..8].copy_from_slice(&now.to_be_bytes());
    getrandom::fill(&mut stream[8..]).map_err(|_| Error::Identity("random"))?;
    let binding = Binding {
        stream,
        session,
        client,
        server: remote,
    };
    let codec = if compress { Codec::Zlib } else { Codec::None };
    let launch = Launch {
        version: protocol::VERSION,
        mode,
        transfer,
        client,
        server: remote,
        direction,
        source,
        destination,
        overwrite,
        preserve_mode,
        preserve_mtime,
    };
    launch.validate().map_err(|_| invalid("copy_launch"))?;
    diagnostic(format_args!(
        "marmish copy: transfer {} stream {}",
        hex::encode(transfer),
        hex::encode(stream)
    ));
    let context = Arc::new(context);
    let mut session = StreamSession::new(context.clone(), relays, binding.clone(), idle, codec);
    let control = session.control();
    let local = Arc::new(AtomicBool::new(false));
    let mut remote_finished = false;
    let mut lease = None;
    let mut worker = None;
    let operation = async {
        if launch.direction == Direction::Get {
            lease = Some(Lease::reserve(&context, &launch, &binding).await?);
        }
        open(&mut session, &launch, codec, probe).await?;
        diagnostic(format_args!(
            "marmish copy: transport codec {}",
            match codec {
                Codec::None => "none",
                Codec::Zlib => "zlib",
            }
        ));
        let mut io = CopyIo::new(session, codec);
        io.set_keepalive(keepalive);
        if let Some(receiver) = &lease
            && let Err(error) = receiver.activate().await
        {
            if let Error::Copy(code) = &error {
                let _ = io.abort_failure(*code).await;
                remote_finished = io.cleanup_confirmed();
            }
            return Err(error);
        }
        let receiver_base = lease.as_ref().map(Lease::base);
        let retention = lease.as_ref().map_or(retention, Lease::retention);
        worker = Some(Worker::start_at(
            Cancellation::Owned(local.clone()),
            receiver_base,
        ));
        let result = crate::copy_client::run_with_worker(
            &mut io,
            launch,
            stream,
            retention,
            local.clone(),
            worker.as_ref().unwrap(),
        )
        .await;
        remote_finished = io.cleanup_confirmed();
        result?;
        diagnostic(format_args!(
            "marmish copy: committed and verified transfer {}",
            hex::encode(transfer)
        ));
        Ok(0)
    };
    let mut result = async { tokio::select! {
        biased;
        signal=signals.interrupted()=>{signal?;cancel(&control,&mut signals,&local).await},
        result=operation=>match result {
            Err(_error) if signals.pending_after_dispatch().await?=>cancel(&control,&mut signals,&local).await,
            result=>result,
        },
    }}.await;
    // Keep worker ownership outside the selected protocol future. Cancellation
    // or attachment loss cannot release an index claim before its physical lock.
    local.store(true, Ordering::Release);
    let disposition = match worker {
        Some(worker) => worker.shutdown(Duration::from_secs(15)).await,
        None => Ok(Disposition::NeverSubmitted),
    };
    match disposition {
        Ok(disposition) => {
            if let Some(receiver) = lease
                && let Err(error) = receiver.release(disposition).await
            {
                diagnostic(format_args!(
                    "marmish copy: local_receiver_cleanup_unconfirmed"
                ));
                if result.is_ok() {
                    result = Err(error);
                }
            }
        }
        Err(error) => {
            diagnostic(format_args!(
                "marmish copy: local_receiver_cleanup_unconfirmed"
            ));
            if result.is_ok() {
                result = Err(error);
            }
        }
    }
    if result.is_err() && control.open_attempted() && !remote_finished {
        diagnostic(format_args!(
            "marmish copy: attachment ended; remote stream {} may continue; use cancel --stream {} to request cleanup",
            hex::encode(stream),
            hex::encode(stream)
        ));
    }
    result
}

pub(crate) fn failure_exit(error: &Error) -> u8 {
    match error {
        Error::Copy(protocol::CopyError::CommitUncertain)
        | Error::Stream(marmish_stream::wire::WireErrorCode::Uncertain)
        | Error::Timeout => 255,
        Error::Copy(_) | Error::Stream(_) => 1,
        Error::Invalid(
            "recovery_required"
            | "copy_capability_unavailable"
            | "copy_path"
            | "copy_transfer_id"
            | "copy_retention"
            | "copy_keepalive"
            | "copy_probe_timeout",
        ) => 1,
        _ => 255,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn finite_failures_and_uncertain_outcomes_have_distinct_exit_codes() {
        for code in [
            protocol::CopyError::NotFound,
            protocol::CopyError::Conflict,
            protocol::CopyError::PermissionDenied,
            protocol::CopyError::Protocol,
        ] {
            assert_eq!(failure_exit(&Error::Copy(code)), 1);
        }
        assert_eq!(
            failure_exit(&Error::Copy(protocol::CopyError::CommitUncertain)),
            255
        );
        assert_eq!(failure_exit(&Error::Timeout), 255);
        assert_eq!(failure_exit(&invalid("recovery_required")), 1);
        assert_eq!(
            failure_exit(&Error::Stream(marmish_stream::wire::WireErrorCode::Denied)),
            1
        );
        assert_eq!(failure_exit(&invalid("stream_cancellation_uncertain")), 255);
    }
    #[test]
    fn admission_refusal_is_distinct_from_filesystem_and_commit_failure() {
        use marmish_stream::wire::CopyAdmissionError;
        let denied = admission_failure(CopyAdmissionError::Denied);
        assert!(matches!(
            denied,
            Error::Stream(marmish_stream::wire::WireErrorCode::Denied)
        ));
        assert_eq!(failure_exit(&denied), 1);
        assert!(denied.to_string().contains("authorization_denied"));
        let recovery = admission_failure(CopyAdmissionError::RecoveryRequired);
        assert_eq!(failure_exit(&recovery), 1);
        assert!(recovery.to_string().contains("recovery_required"));
        assert_eq!(
            failure_exit(&Error::Copy(protocol::CopyError::CommitUncertain)),
            255
        );
    }
    #[test]
    fn resume_id_is_strict_and_create_is_distinct() {
        assert!(transfer_id(Some(&"ab".repeat(32))).is_ok());
        for value in [
            "AB".repeat(32),
            "g0".repeat(32),
            "a".repeat(63),
            "a".repeat(65),
        ] {
            assert!(transfer_id(Some(&value)).is_err());
        }
        let (a, mode) = transfer_id(None).unwrap();
        let (b, _) = transfer_id(None).unwrap();
        assert_ne!(a, b);
        assert_eq!(mode, AttemptMode::Create);
    }
}
