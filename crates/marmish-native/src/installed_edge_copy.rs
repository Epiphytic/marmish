//! Installed tiny-copy edge cases. Ordinary preparation stays inside owned workspaces.
use super::{
    copy_checkpoint,
    copy_interrupt::Binding,
    edge_signals::Cohort,
    matrix, process, records,
    streaming_cleanup::{self, StreamStatus},
    transfer_cases,
};
use crate::{Error, Result};
use marmish_stream::copy_protocol::{
    self as protocol, AttemptMode, Checkpoint, Direction, FileIdentity, Launch, Phase,
};
use serde::{Deserialize, Serialize};
use std::{
    ffi::{CStr, CString, OsString},
    fs::{File, FileTimes, Metadata, OpenOptions},
    io::Read,
    os::fd::{AsRawFd, FromRawFd},
    os::unix::{
        ffi::OsStrExt,
        fs::{DirBuilderExt, FileExt, MetadataExt, OpenOptionsExt, PermissionsExt, symlink},
    },
    path::{Path, PathBuf},
    time::{Duration, UNIX_EPOCH},
};
use tokio::time::Instant;

const CALL_SECONDS: u64 = 180;
const LIMIT_NS: u64 = CALL_SECONDS * 1_000_000_000;
fn bad() -> Error {
    Error::Invalid("installed_edge_copy")
}
#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum Case {
    Empty,
    ExistingRefusal,
    Overwrite,
    ReceiptSeed,
    SymlinkSource,
    DirectorySource,
    DestinationSymlink,
    DestinationDirectory,
    ResumeChangedSource,
    ResumeChangedBinding,
    ResumeCorruptCheckpoint,
}
impl Case {
    fn diagnostic(self) -> Option<&'static str> {
        match self {
            Self::Empty | Self::Overwrite | Self::ReceiptSeed => None,
            Self::ExistingRefusal | Self::ResumeChangedSource | Self::ResumeChangedBinding => {
                Some("transfer_conflict")
            }
            Self::SymlinkSource
            | Self::DirectorySource
            | Self::DestinationSymlink
            | Self::DestinationDirectory => Some("unsafe_path"),
            Self::ResumeCorruptCheckpoint => Some("copy_protocol_error"),
        }
    }
    fn kind(self) -> &'static str {
        match self {
            Self::ReceiptSeed => "completed-receipt-seed",
            Self::ResumeChangedSource
            | Self::ResumeChangedBinding
            | Self::ResumeCorruptCheckpoint => "completed-receipt-negative",
            _ => "ordinary",
        }
    }
    fn name(self) -> &'static str {
        match self {
            Self::Empty => "empty",
            Self::ExistingRefusal => "existing-refusal",
            Self::Overwrite => "overwrite",
            Self::ReceiptSeed => "receipt-seed",
            Self::SymlinkSource => "symlink-source",
            Self::DirectorySource => "directory-source",
            Self::DestinationSymlink => "destination-symlink",
            Self::DestinationDirectory => "destination-directory",
            Self::ResumeChangedSource => "resume-changed-source",
            Self::ResumeChangedBinding => "resume-changed-binding",
            Self::ResumeCorruptCheckpoint => "resume-corrupt-checkpoint",
        }
    }
    fn failure(self) -> &'static str {
        match self {
            Self::Empty => "installed_copy_edge_empty",
            Self::ExistingRefusal => "installed_copy_edge_existing_refusal",
            Self::Overwrite => "installed_copy_edge_overwrite",
            Self::ReceiptSeed => "installed_copy_edge_receipt_seed",
            Self::SymlinkSource => "installed_copy_edge_symlink",
            Self::DirectorySource => "installed_copy_edge_directory",
            Self::DestinationSymlink => "installed_copy_edge_destination_symlink",
            Self::DestinationDirectory => "installed_copy_edge_destination_directory",
            Self::ResumeChangedSource => "installed_copy_edge_resume_source",
            Self::ResumeChangedBinding => "installed_copy_edge_resume_binding",
            Self::ResumeCorruptCheckpoint => "installed_copy_edge_resume_corrupt",
        }
    }
    fn preserves(self) -> bool {
        self.diagnostic().is_some()
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Observation {
    schema: u8,
    case: Case,
    kind: String,
    direction: String,
    wall_ns: u64,
    exit: i32,
    diagnostic: Option<String>,
    binding: Option<Binding>,
    baseline: Option<Binding>,
    remote_terminal: Option<marmish_stream::wire::Terminal>,
    transfer_sha256: String,
    before_sha256: Option<String>,
    after_sha256: String,
    exact_metadata: bool,
    target_entry_preserved: Option<bool>,
    /// Present only when actual installed copy produced the inspected receipt.
    receipt: Option<copy_checkpoint::Proof>,
    client_cleanup: StreamStatus,
    server_cleanup: StreamStatus,
}
impl Observation {
    pub(super) fn key(&self) -> String {
        format!("{}-{}", self.direction, self.case.name())
    }
    pub(super) fn bindings(&self) -> Vec<&Binding> {
        self.binding.iter().chain(self.baseline.iter()).collect()
    }
    pub(super) fn wall_ns(&self) -> u64 {
        self.wall_ns
    }
    pub(super) fn remote_terminal(&self) -> Option<marmish_stream::wire::Terminal> {
        self.remote_terminal
    }
    pub(super) fn validate(&self) -> Result<()> {
        self.client_cleanup.validate_idle()?;
        self.server_cleanup.validate_idle()?;
        if self.schema != 1
            || self.kind != self.case.kind()
            || !matches!(self.direction.as_str(), "put" | "get")
            || self.wall_ns == 0
            || self.wall_ns > LIMIT_NS
            || !self.exact_metadata
            || self.target_entry_preserved
                != if matches!(
                    self.case,
                    Case::DestinationSymlink | Case::DestinationDirectory
                ) {
                    Some(true)
                } else {
                    None
                }
            || self.exit != i32::from(self.case.diagnostic().is_some())
            || self.diagnostic.as_deref() != self.case.diagnostic()
            || self.binding.as_ref().is_some_and(|b| {
                [
                    &b.stream_sha256,
                    &b.session_sha256,
                    &b.open_digest,
                    &b.group_sha256,
                ]
                .into_iter()
                .any(|s| !records::hexadecimal(s, 64))
            })
            || (self.binding.is_none() != matches!(self.case, Case::ResumeChangedBinding))
            || self.remote_terminal
                != self
                    .binding
                    .as_ref()
                    .map(|_| marmish_stream::wire::Terminal::Exited {
                        code: u8::from(self.case.diagnostic().is_some()),
                    })
            || [&self.transfer_sha256, &self.after_sha256]
                .into_iter()
                .any(|s| !records::hexadecimal(s, 64))
            || self
                .before_sha256
                .as_ref()
                .is_some_and(|s| !records::hexadecimal(s, 64))
            || (self.case.preserves() && self.before_sha256.as_ref() != Some(&self.after_sha256))
            || (matches!(self.case, Case::Empty) && self.after_sha256 != records::digest(&[]))
        {
            return Err(bad());
        }
        let resume = matches!(
            self.case,
            Case::ResumeChangedSource | Case::ResumeChangedBinding | Case::ResumeCorruptCheckpoint
        );
        if (self.receipt.is_some() != (resume || matches!(self.case, Case::ReceiptSeed)))
            || (self.baseline.is_some() != resume)
        {
            return Err(bad());
        }
        if let Some(b) = &self.baseline
            && ([
                &b.stream_sha256,
                &b.session_sha256,
                &b.open_digest,
                &b.group_sha256,
            ]
            .into_iter()
            .any(|s| !records::hexadecimal(s, 64))
                || self
                    .receipt
                    .as_ref()
                    .is_none_or(|p| p.stream_sha256 != b.stream_sha256))
        {
            return Err(bad());
        }
        if let Some(p) = &self.receipt
            && (!matches!(
                self.case,
                Case::ReceiptSeed
                    | Case::ResumeChangedSource
                    | Case::ResumeChangedBinding
                    | Case::ResumeCorruptCheckpoint
            ) || p.phase != "committed"
                || p.durable_offset != p.size
                || p.prefix_sha256 != p.sha256
                || p.sha256 != self.after_sha256
                || p.transfer_sha256 != self.transfer_sha256
                || [
                    &p.transfer_sha256,
                    &p.stream_sha256,
                    &p.manifest_sha256,
                    &p.prefix_sha256,
                    &p.sha256,
                ]
                .into_iter()
                .any(|x| !records::hexadecimal(x, 64))
                || self.binding.as_ref().is_some_and(|b| {
                    (b.stream_sha256 == p.stream_sha256) != matches!(self.case, Case::ReceiptSeed)
                }))
        {
            return Err(bad());
        }
        Ok(())
    }
}
fn private(parent: &Path, name: &str) -> Result<PathBuf> {
    crate::identity::check_dir(parent)?;
    if parent.canonicalize()? != parent {
        return Err(bad());
    }
    let path = parent.join(name);
    std::fs::DirBuilder::new().mode(0o700).create(&path)?;
    crate::identity::check_dir(&path)?;
    Ok(path)
}
fn payload(path: &Path, bytes: &[u8]) -> Result<()> {
    std::fs::write(path, bytes)?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o640))?;
    File::open(path)?.set_times(
        FileTimes::new().set_modified(UNIX_EPOCH + Duration::new(1_700_000_000, 123_456_789)),
    )?;
    Ok(())
}
fn metadata_equal(a: &Metadata, b: &Metadata) -> bool {
    (a.mode() & 0o7777, a.mtime(), a.mtime_nsec()) == (b.mode() & 0o7777, b.mtime(), b.mtime_nsec())
}
fn preserved_metadata(a: &Metadata, b: &Metadata) -> bool {
    metadata_equal(a, b)
        && file_identity(a) == file_identity(b)
        && (a.nlink(), a.uid(), a.gid(), a.len()) == (b.nlink(), b.uid(), b.gid(), b.len())
}
fn absent_entry(path: &Path) -> Result<bool> {
    match std::fs::symlink_metadata(path) {
        Ok(_) => Ok(false),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(true),
        Err(error) => Err(error.into()),
    }
}
fn case_error(case: Case, error: Error) -> Error {
    if matches!(error, Error::Timeout) {
        error
    } else {
        Error::Invalid(case.failure())
    }
}
fn validate_refusal_records(
    entries: &[streaming_cleanup::Entry],
    stream: &str,
    client: &str,
    server: &str,
    endpoint: &str,
    required: bool,
) -> Result<()> {
    let matching: Vec<_> = entries
        .iter()
        .filter(|entry| entry.stream == stream)
        .collect();
    if matching.len() > 1
        || (required && matching.is_empty())
        || matching.iter().any(|entry| {
            entry.active
                || entry.open_digest.is_some()
                || entry.cancel_requested
                || entry.client != client
                || entry.server != server
                || entry.direction != endpoint
        })
    {
        return Err(bad());
    }
    Ok(())
}
struct Attempt<'a> {
    direction: &'a str,
    source: &'a Path,
    destination: &'a Path,
    overwrite: bool,
    resume: Option<&'a str>,
    protected: Option<&'a Path>,
}
struct Completed {
    observation: Observation,
    transfer: String,
    stream: String,
}
fn ids(stderr: &[u8], case: Case, direction: &str) -> Result<(String, String)> {
    if case.diagnostic().is_none() {
        let (stream, transfer) =
            transfer_cases::copy_ids_codec(stderr, marmish_stream::codec::Codec::None)?;
        return Ok((stream, transfer));
    }
    let text = std::str::from_utf8(stderr).map_err(|_| bad())?;
    let mut lines = text.lines();
    let (transfer, stream) = lines
        .next()
        .and_then(|line| line.strip_prefix("marmish copy: transfer "))
        .and_then(|line| line.split_once(" stream "))
        .ok_or_else(bad)?;
    if !records::hexadecimal(transfer, 64)
        || !records::hexadecimal(stream, 32)
        || (!matches!(case, Case::ResumeChangedBinding)
            && lines.next() != Some("marmish copy: transport codec none"))
        || (matches!(case, Case::ResumeChangedBinding) && direction == "put" && (lines.next() != Some("marmish copy: admission Conflict") || lines.next() != Some(format!("marmish copy: attachment ended; remote stream {stream} may continue; use cancel --stream {stream} to request cleanup").as_str())))
        || lines.next() != Some(format!("marmish: copy: {}", case.diagnostic().unwrap()).as_str())
        || lines.next().is_some()
    {
        return Err(bad());
    }
    Ok((stream.into(), transfer.into()))
}
async fn idle(c: &Cohort<'_, '_>) -> Result<(StreamStatus, StreamStatus)> {
    loop {
        if Instant::now() >= c.deadline {
            return Err(Error::Timeout);
        }
        let client =
            streaming_cleanup::status(*c.client.daemon.lock().await, c.client.socket, c.deadline)
                .await?;
        let server =
            streaming_cleanup::status(*c.server.daemon.lock().await, c.server.socket, c.deadline)
                .await?;
        if client.validate_idle().is_ok() && server.validate_idle().is_ok() {
            return Ok((client, server));
        }
        tokio::time::sleep_until((Instant::now() + Duration::from_millis(50)).min(c.deadline))
            .await;
    }
}
fn attempt_args(account: &str, attempt: &Attempt<'_>) -> Result<Vec<OsString>> {
    let mut args = Vec::new();
    args.extend([
        "--no-autostart".into(),
        attempt.direction.into(),
        "--preserve-mode".into(),
        "--preserve-mtime".into(),
    ]);
    if attempt.overwrite {
        args.push("--overwrite".into());
    }
    if let Some(transfer) = attempt.resume {
        args.extend(["--resume".into(), transfer.into()]);
    }
    args.extend([
        "--".into(),
        records::public_destination(account)?.into(),
        attempt.source.as_os_str().to_owned(),
        attempt.destination.as_os_str().to_owned(),
    ]);
    Ok(args)
}
async fn invoke(c: &Cohort<'_, '_>, case: Case, attempt: Attempt<'_>) -> Result<Completed> {
    let scoped = Cohort {
        context: c.context,
        identities: c.identities,
        role: c.role,
        ready: c.ready,
        client: c.client,
        server: c.server,
        deadline: c
            .deadline
            .min(Instant::now() + Duration::from_secs(CALL_SECONDS)),
    };
    invoke_inner(&scoped, case, attempt)
        .await
        .map_err(|error| case_error(case, error))
}
async fn invoke_inner(c: &Cohort<'_, '_>, case: Case, attempt: Attempt<'_>) -> Result<Completed> {
    let protected = attempt.protected.unwrap_or(attempt.destination);
    let before = protected
        .try_exists()?
        .then(|| {
            Ok::<_, Error>((
                records::digest(&std::fs::read(protected)?),
                std::fs::metadata(protected)?,
            ))
        })
        .transpose()?;
    let mut args = matrix::base(
        c.client.workspace,
        "client",
        c.client.socket,
        &c.context.relays,
    );
    args.extend(attempt_args(&c.ready.server_account, &attempt)?);
    let start = Instant::now();
    let out = process::run(
        &c.client.workspace.join("bin/marmish"),
        &args,
        c.client.workspace,
        c.client.environment,
        c.deadline
            .min(Instant::now() + Duration::from_secs(CALL_SECONDS)),
        65536,
    )
    .await?;
    let (stream, transfer) = ids(&out.stderr, case, attempt.direction)?;
    if !out.stdout.is_empty() || out.status.code() != Some(i32::from(case.diagnostic().is_some())) {
        return Err(bad());
    }
    let (client_cleanup, server_cleanup) = idle(c).await?;
    let (binding, remote_terminal) = if matches!(case, Case::ResumeChangedBinding) {
        // A hello-only record is allowed, but never an accepted Open.
        for (endpoint, label) in [(c.client, "client"), (c.server, "server")] {
            let entries = streaming_cleanup::entries(
                *endpoint.daemon.lock().await,
                endpoint.socket,
                c.deadline,
            )
            .await?;
            validate_refusal_records(
                &entries,
                &stream,
                &c.identities[c.role].client_account,
                &c.ready.server_account,
                label,
                attempt.direction == "put",
            )?;
        }
        (None, None)
    } else {
        let entries =
            streaming_cleanup::entries(*c.client.daemon.lock().await, c.client.socket, c.deadline)
                .await?;
        let entry = entries
            .into_iter()
            .find(|e| e.stream == stream)
            .ok_or_else(bad)?;
        if entry.direction != "client"
            || entry.client != c.identities[c.role].client_account
            || entry.server != c.ready.server_account
        {
            return Err(bad());
        }
        let server_entry =
            streaming_cleanup::entries(*c.server.daemon.lock().await, c.server.socket, c.deadline)
                .await?
                .into_iter()
                .find(|e| e.stream == stream)
                .ok_or_else(bad)?;
        if server_entry.direction != "server"
            || server_entry.client != entry.client
            || server_entry.server != entry.server
            || server_entry.session != entry.session
            || server_entry.group_sha256 != entry.group_sha256
            || server_entry.open_digest != entry.open_digest
            || server_entry.active
            || server_entry.terminal.is_none()
            || (case.diagnostic().is_none()
                && server_entry.terminal
                    != Some(marmish_stream::wire::Terminal::Exited { code: 0 }))
        {
            return Err(bad());
        }
        (
            Some(Binding {
                stream_sha256: records::digest(stream.as_bytes()),
                session_sha256: records::digest(entry.session.as_bytes()),
                open_digest: entry.open_digest.ok_or_else(bad)?,
                group_sha256: entry.group_sha256,
            }),
            server_entry.terminal,
        )
    };
    let after = std::fs::metadata(protected)?;
    let after_sha256 = records::digest(&std::fs::read(protected)?);
    let exact_metadata = if case.preserves() {
        before
            .as_ref()
            .is_some_and(|(_, m)| preserved_metadata(m, &after))
    } else {
        metadata_equal(&std::fs::metadata(attempt.source)?, &after)
    };
    let observation = Observation {
        schema: 1,
        case,
        kind: case.kind().into(),
        direction: attempt.direction.into(),
        wall_ns: start.elapsed().as_nanos().try_into().map_err(|_| bad())?,
        exit: out.status.code().ok_or_else(bad)?,
        diagnostic: case.diagnostic().map(str::to_owned),
        binding,
        baseline: None,
        remote_terminal,
        transfer_sha256: records::digest(transfer.as_bytes()),
        before_sha256: before.map(|(hash, _)| hash),
        after_sha256,
        exact_metadata,
        target_entry_preserved: None,
        receipt: None,
        client_cleanup,
        server_cleanup,
    };
    if !matches!(
        case,
        Case::ReceiptSeed
            | Case::DestinationSymlink
            | Case::DestinationDirectory
            | Case::ResumeChangedSource
            | Case::ResumeChangedBinding
            | Case::ResumeCorruptCheckpoint
    ) {
        observation.validate()?;
    }
    Ok(Completed {
        observation,
        transfer,
        stream,
    })
}
pub(super) async fn run(mut c: Cohort<'_, '_>) -> Result<Vec<Observation>> {
    c.deadline = c.deadline.min(Instant::now() + Duration::from_secs(1140));
    let nonce = records::digest(&records::canonical(&(
        "installed-copy-edge/1",
        c.context.digest()?,
        c.role,
        &c.ready.server_account,
    ))?);
    let local = private(c.client.workspace, &format!("edge-copy-{nonce}"))?;
    let remote = private(c.server.workspace, &format!("edge-copy-{nonce}"))?;
    let mut observations = Vec::new();
    for direction in ["put", "get"] {
        let client = private(&local, direction)?;
        let server = private(&remote, direction)?;
        let (send, receive) = if direction == "put" {
            (&client, &server)
        } else {
            (&server, &client)
        };
        let source = send.join("source");
        let empty = send.join("empty");
        payload(&source, b"tiny copy edge payload\0\xff")?;
        payload(&empty, &[])?;
        let empty_target = receive.join("empty");
        observations.push(
            invoke(
                &c,
                Case::Empty,
                Attempt {
                    direction,
                    source: &empty,
                    destination: &empty_target,
                    overwrite: false,
                    resume: None,
                    protected: None,
                },
            )
            .await?
            .observation,
        );
        let target = receive.join("existing");
        payload(&target, b"existing sentinel must survive failure\0")?;
        observations.push(
            invoke(
                &c,
                Case::ExistingRefusal,
                Attempt {
                    direction,
                    source: &source,
                    destination: &target,
                    overwrite: false,
                    resume: None,
                    protected: None,
                },
            )
            .await?
            .observation,
        );
        observations.push(
            invoke(
                &c,
                Case::Overwrite,
                Attempt {
                    direction,
                    source: &source,
                    destination: &target,
                    overwrite: true,
                    resume: None,
                    protected: None,
                },
            )
            .await?
            .observation,
        );
        if std::fs::read(&target)? != std::fs::read(&source)? {
            return Err(bad());
        }
        let alias = send.join("alias");
        symlink(&source, &alias)?;
        let directory = private(send, "directory")?;
        for (case, operand) in [
            (Case::SymlinkSource, &alias),
            (Case::DirectorySource, &directory),
        ] {
            observations.push(
                invoke(
                    &c,
                    case,
                    Attempt {
                        direction,
                        source: operand,
                        destination: &target,
                        overwrite: true,
                        resume: None,
                        protected: None,
                    },
                )
                .await?
                .observation,
            );
        }
        destination_cases(&c, direction, &source, receive, &mut observations).await?;
        receipt_cases(&c, direction, send, receive, &mut observations).await?;
    }
    if observations
        .iter()
        .map(Observation::key)
        .collect::<std::collections::BTreeSet<_>>()
        != expected_keys().into_iter().collect()
        || observations.len() != 22
    {
        return Err(bad());
    }
    for observation in &observations {
        observation.validate()?;
    }
    Ok(observations)
}

pub(super) fn expected_keys() -> Vec<String> {
    ["put", "get"]
        .into_iter()
        .flat_map(|direction| {
            [
                Case::Empty,
                Case::ExistingRefusal,
                Case::Overwrite,
                Case::SymlinkSource,
                Case::DirectorySource,
                Case::DestinationSymlink,
                Case::DestinationDirectory,
                Case::ReceiptSeed,
                Case::ResumeChangedSource,
                Case::ResumeChangedBinding,
                Case::ResumeCorruptCheckpoint,
            ]
            .into_iter()
            .map(move |case| format!("{direction}-{}", case.name()))
        })
        .collect()
}
fn lstat_at(parent: &File, name: &[u8]) -> Result<libc::stat> {
    if name.is_empty() || name.contains(&b'/') {
        return Err(bad());
    }
    let name = CString::new(name).map_err(|_| bad())?;
    let mut st = std::mem::MaybeUninit::<libc::stat>::uninit();
    if unsafe {
        libc::fstatat(
            parent.as_raw_fd(),
            name.as_ptr(),
            st.as_mut_ptr(),
            libc::AT_SYMLINK_NOFOLLOW,
        )
    } != 0
    {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(unsafe { st.assume_init() })
}
fn same_stat(a: &libc::stat, b: &libc::stat) -> bool {
    let nanos = a.st_mtime_nsec == b.st_mtime_nsec;
    a.st_dev == b.st_dev
        && a.st_ino == b.st_ino
        && a.st_nlink == b.st_nlink
        && a.st_mode == b.st_mode
        && a.st_uid == b.st_uid
        && a.st_size == b.st_size
        && a.st_mtime == b.st_mtime
        && nanos
}
fn link_at(parent: &File, name: &[u8]) -> Result<Vec<u8>> {
    let name = CString::new(name).map_err(|_| bad())?;
    let mut bytes = vec![0; 4096];
    let n = unsafe {
        libc::readlinkat(
            parent.as_raw_fd(),
            name.as_ptr(),
            bytes.as_mut_ptr().cast(),
            bytes.len(),
        )
    };
    if n < 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    let n = usize::try_from(n).map_err(|_| bad())?;
    if n == bytes.len() {
        return Err(bad());
    }
    bytes.truncate(n);
    Ok(bytes)
}
fn directory_names(file: &File) -> Result<Vec<Vec<u8>>> {
    let fd = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_DUPFD_CLOEXEC, 0) };
    if fd < 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    let raw = unsafe { libc::fdopendir(fd) };
    if raw.is_null() {
        unsafe { libc::close(fd) };
        return Err(std::io::Error::last_os_error().into());
    }
    struct Directory(*mut libc::DIR);
    impl Drop for Directory {
        fn drop(&mut self) {
            unsafe { libc::closedir(self.0) };
        }
    }
    let directory = Directory(raw);
    let mut names = Vec::new();
    loop {
        #[cfg(target_os = "linux")]
        unsafe {
            *libc::__errno_location() = 0;
        }
        #[cfg(target_os = "macos")]
        unsafe {
            *libc::__error() = 0;
        }
        let entry = unsafe { libc::readdir(directory.0) };
        if entry.is_null() {
            let error = std::io::Error::last_os_error();
            if error.raw_os_error() != Some(0) {
                return Err(error.into());
            }
            break;
        }
        let name = unsafe { CStr::from_ptr((*entry).d_name.as_ptr()) }.to_bytes();
        if name != b"." && name != b".." {
            names.push(name.to_vec());
            if names.len() > 4 {
                return Err(bad());
            }
        }
    }
    names.sort();
    Ok(names)
}
struct DestinationProof {
    entry: libc::stat,
    sentinel: Metadata,
    link: Option<Vec<u8>>,
    names: Option<Vec<Vec<u8>>>,
}
fn destination_proof(parent: &Path, case: Case) -> Result<DestinationProof> {
    let root = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(parent)?;
    checked(&root, true)?;
    let entry = lstat_at(&root, b"destination")?;
    let (sentinel, link, names) = if matches!(case, Case::DestinationSymlink) {
        (
            open_at(&root, b"sentinel", false, false)?,
            Some(link_at(&root, b"destination")?),
            None,
        )
    } else {
        let destination = open_at(&root, b"destination", true, false)?;
        let names = directory_names(&destination)?;
        if names != [b"sentinel".to_vec()] {
            return Err(bad());
        }
        (
            open_at(&destination, b"sentinel", false, false)?,
            None,
            Some(names),
        )
    };
    Ok(DestinationProof {
        entry,
        sentinel: checked(&sentinel, false)?,
        link,
        names,
    })
}
async fn destination_cases(
    c: &Cohort<'_, '_>,
    direction: &str,
    source: &Path,
    receive: &Path,
    observations: &mut Vec<Observation>,
) -> Result<()> {
    for case in [Case::DestinationSymlink, Case::DestinationDirectory] {
        let parent = private(receive, case.name())?;
        let destination = parent.join("destination");
        let sentinel = if matches!(case, Case::DestinationSymlink) {
            let sentinel = parent.join("sentinel");
            symlink(&sentinel, &destination)?;
            sentinel
        } else {
            private(&parent, "destination")?.join("sentinel")
        };
        payload(&sentinel, b"destination sentinel must survive\0\xff")?;
        std::fs::set_permissions(&sentinel, std::fs::Permissions::from_mode(0o600))?;
        let before = destination_proof(&parent, case)?;
        let mut result = invoke(
            c,
            case,
            Attempt {
                direction,
                source,
                destination: &destination,
                overwrite: true,
                resume: None,
                protected: Some(&sentinel),
            },
        )
        .await?;
        let after = destination_proof(&parent, case)?;
        if !same_stat(&before.entry, &after.entry)
            || file_identity(&before.sentinel) != file_identity(&after.sentinel)
            || before.sentinel.nlink() != after.sentinel.nlink()
            || before.link != after.link
            || before.names != after.names
        {
            return Err(Error::Invalid(case.failure()));
        }
        result.observation.target_entry_preserved = Some(true);
        result.observation.validate()?;
        observations.push(result.observation);
    }
    Ok(())
}

fn fixed<const N: usize>(text: &str) -> Result<[u8; N]> {
    hex::decode(text)
        .map_err(|_| bad())?
        .try_into()
        .map_err(|_| bad())
}
fn expected<'a>(
    parent: &'a Path,
    launch: &'a Launch,
    stream: [u8; 16],
    bytes: &[u8],
) -> Result<copy_checkpoint::Expected<'a>> {
    Ok(copy_checkpoint::Expected {
        parent,
        launch,
        stream,
        size: bytes.len().try_into().map_err(|_| bad())?,
        sha256: fixed(&records::digest(bytes))?,
        committed: true,
    })
}
fn file_identity(m: &Metadata) -> FileIdentity {
    FileIdentity {
        device: m.dev(),
        inode: m.ino(),
    }
}
fn checked(file: &File, directory: bool) -> Result<Metadata> {
    let m = file.metadata()?;
    if m.uid() != unsafe { libc::geteuid() }
        || if directory {
            !m.is_dir() || m.mode() & 0o7777 != 0o700
        } else {
            !m.is_file() || m.mode() & 0o7777 != 0o600 || m.nlink() != 1
        }
    {
        return Err(bad());
    }
    Ok(m)
}
fn open_at(parent: &File, name: &[u8], directory: bool, write: bool) -> Result<File> {
    if name.is_empty() || name.contains(&b'/') || (directory && write) {
        return Err(bad());
    }
    let name = CString::new(name).map_err(|_| bad())?;
    let flags = (if write { libc::O_RDWR } else { libc::O_RDONLY })
        | libc::O_CLOEXEC
        | libc::O_NOFOLLOW
        | libc::O_NONBLOCK
        | if directory { libc::O_DIRECTORY } else { 0 };
    let fd = unsafe { libc::openat(parent.as_raw_fd(), name.as_ptr(), flags) };
    if fd < 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    let file = unsafe { File::from_raw_fd(fd) };
    checked(&file, directory)?;
    Ok(file)
}
/// Negative fixture only. The caller has already proven both daemons idle and
/// observed an actual completed installed transfer. This grants no GC authority.
fn corrupt_committed(
    e: copy_checkpoint::Expected<'_>,
    proof: &copy_checkpoint::Proof,
) -> Result<()> {
    if copy_checkpoint::observe(copy_checkpoint::Expected {
        parent: e.parent,
        launch: e.launch,
        stream: e.stream,
        size: e.size,
        sha256: e.sha256,
        committed: true,
    })? != *proof
    {
        return Err(bad());
    }
    let parent = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(e.parent)?;
    let parent_id = file_identity(&checked(&parent, true)?);
    let private = open_at(&parent, e.launch.namespace().as_bytes(), true, false)?;
    let private_id = file_identity(&checked(&private, true)?);
    let mut file = open_at(&private, b"checkpoint", false, true)?;
    let before = checked(&file, false)?;
    if before.len() == 0 || before.len() > protocol::MAX_CONTROL as u64 {
        return Err(bad());
    }
    let mut bytes = Vec::new();
    Read::by_ref(&mut file)
        .take(protocol::MAX_CONTROL as u64 + 1)
        .read_to_end(&mut bytes)?;
    let cp = Checkpoint::decode(&bytes).map_err(|_| bad())?;
    if bytes.len() as u64 != before.len()
        || cp.parent != parent_id
        || cp.private != private_id
        || cp.phase != Phase::Committed
        || cp.last_stream != e.stream
        || !e.launch.matches_manifest(&cp.manifest)
        || cp.manifest.size != e.size
        || cp.manifest.sha256 != e.sha256
        || cp.normalized_path != e.launch.destination
    {
        return Err(bad());
    }
    // pwrite cannot follow a later pathname replacement. Recheck the entry inode
    // as well so the evidence names exactly the descriptor that was mutated.
    if file_identity(&checked(
        &open_at(&private, b"checkpoint", false, false)?,
        false,
    )?) != file_identity(&before)
    {
        return Err(bad());
    }
    bytes[0] ^= 1;
    file.write_all_at(&bytes[..1], 0)?;
    file.sync_all()?;
    let mut after = vec![0; bytes.len()];
    file.read_exact_at(&mut after, 0)?;
    if after != bytes
        || Checkpoint::decode(&after).is_ok()
        || checked(&file, false)?.len() != before.len()
        || file_identity(&checked(
            &open_at(&private, b"checkpoint", false, false)?,
            false,
        )?) != file_identity(&before)
    {
        return Err(bad());
    }
    Ok(())
}
async fn receipt_cases(
    c: &Cohort<'_, '_>,
    direction: &str,
    send: &Path,
    receive: &Path,
    observations: &mut Vec<Observation>,
) -> Result<()> {
    // Separate parents keep corrupt retained state out of ordinary inventories.
    let source_parent = private(send, "receipt-negative")?;
    let receiver_parent = private(receive, "receipt-negative")?;
    let source = source_parent.join("source");
    let target = receiver_parent.join("target");
    let bytes = b"completed receipt binary\0\xff";
    payload(&source, bytes)?;
    std::fs::set_permissions(&source, std::fs::Permissions::from_mode(0o600))?;
    let mut seed = invoke(
        c,
        Case::ReceiptSeed,
        Attempt {
            direction,
            source: &source,
            destination: &target,
            overwrite: true,
            resume: None,
            protected: None,
        },
    )
    .await?;
    let launch = Launch {
        version: protocol::VERSION,
        mode: AttemptMode::Create,
        transfer: fixed(&seed.transfer)?,
        client: fixed(&c.identities[c.role].client_account)?,
        server: fixed(&c.ready.server_account)?,
        direction: if direction == "put" {
            Direction::Put
        } else {
            Direction::Get
        },
        source: source.as_os_str().as_bytes().to_vec(),
        destination: target.as_os_str().as_bytes().to_vec(),
        overwrite: true,
        preserve_mode: true,
        preserve_mtime: true,
    };
    let stream = fixed(&seed.stream)?;
    let proof = copy_checkpoint::observe(expected(&receiver_parent, &launch, stream, bytes)?)?;
    let baseline = seed.observation.binding.clone().ok_or_else(bad)?;
    seed.observation.receipt = Some(proof.clone());
    seed.observation.validate()?;
    observations.push(seed.observation);
    for case in [
        Case::ResumeChangedSource,
        Case::ResumeChangedBinding,
        Case::ResumeCorruptCheckpoint,
    ] {
        let deadline = c
            .deadline
            .min(Instant::now() + Duration::from_secs(CALL_SECONDS));
        let scoped = Cohort {
            context: c.context,
            identities: c.identities,
            role: c.role,
            ready: c.ready,
            client: c.client,
            server: c.server,
            deadline,
        };
        if copy_checkpoint::observe(expected(&receiver_parent, &launch, stream, bytes)?)
            .map_err(|error| case_error(case, error))?
            != proof
        {
            return Err(Error::Invalid(case.failure()));
        }
        let alternate = receiver_parent.join("alternate-never-created");
        let destination = if matches!(case, Case::ResumeChangedBinding) {
            &alternate
        } else {
            &target
        };
        if !absent_entry(&alternate)? {
            return Err(bad());
        }
        if matches!(case, Case::ResumeChangedSource) {
            let mut changed = bytes.to_vec();
            changed[0] ^= 1;
            payload(&source, &changed)?;
            std::fs::set_permissions(&source, std::fs::Permissions::from_mode(0o600))?;
        }
        if matches!(case, Case::ResumeCorruptCheckpoint) {
            idle(&scoped)
                .await
                .map_err(|error| case_error(case, error))?;
            corrupt_committed(expected(&receiver_parent, &launch, stream, bytes)?, &proof)
                .map_err(|error| case_error(case, error))?;
        }
        let mut result = invoke(
            &scoped,
            case,
            Attempt {
                direction,
                source: &source,
                destination,
                overwrite: true,
                resume: Some(&seed.transfer),
                protected: Some(&target),
            },
        )
        .await?;
        if result.transfer != seed.transfer || !absent_entry(&alternate)? {
            return Err(Error::Invalid(case.failure()));
        }
        result.observation.baseline = Some(baseline.clone());
        result.observation.receipt = Some(proof.clone());
        result.observation.validate()?;
        observations.push(result.observation);
        if !matches!(case, Case::ResumeCorruptCheckpoint) {
            payload(&source, bytes)?;
            std::fs::set_permissions(&source, std::fs::Permissions::from_mode(0o600))?;
            if copy_checkpoint::observe(expected(&receiver_parent, &launch, stream, bytes)?)?
                != proof
            {
                return Err(Error::Invalid(case.failure()));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn refusal_evidence_requires_put_records_and_rejects_wrong_authority() {
        let mut entry = streaming_cleanup::Entry {
            stream: "stream".into(),
            session: "session".into(),
            client: "client".into(),
            server: "server".into(),
            direction: "server".into(),
            state: "hello".into(),
            open_digest: None,
            group_sha256: "group".into(),
            active: false,
            cancel_requested: false,
            terminal: None,
            child_disposition: None,
        };
        assert!(
            validate_refusal_records(&[], "stream", "client", "server", "server", true).is_err()
        );
        assert!(
            validate_refusal_records(&[], "stream", "client", "server", "server", false).is_ok()
        );
        assert!(
            validate_refusal_records(
                &[entry.clone()],
                "stream",
                "client",
                "server",
                "server",
                true
            )
            .is_ok()
        );
        entry.client = "wrong".into();
        assert!(
            validate_refusal_records(&[entry], "stream", "client", "server", "server", true)
                .is_err()
        );
    }
    #[test]
    fn preserved_target_rejects_replacement_and_dangling_alternate_is_not_absent() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("target");
        let replacement = dir.path().join("replacement");
        payload(&target, b"same").unwrap();
        let held = File::open(&target).unwrap();
        let before = held.metadata().unwrap();
        payload(&replacement, b"same").unwrap();
        std::fs::rename(&replacement, &target).unwrap();
        let after = std::fs::metadata(&target).unwrap();
        assert!(metadata_equal(&before, &after));
        assert!(!preserved_metadata(&before, &after));
        let alternate = dir.path().join("alternate");
        assert!(absent_entry(&alternate).unwrap());
        symlink(dir.path().join("missing"), &alternate).unwrap();
        assert!(!absent_entry(&alternate).unwrap());
    }
    #[test]
    fn case_timeout_stays_timeout_and_call_ceiling_is_independent_of_family() {
        assert!(matches!(
            case_error(Case::Empty, Error::Timeout),
            Error::Timeout
        ));
        assert!(matches!(
            case_error(Case::Empty, bad()),
            Error::Invalid("installed_copy_edge_empty")
        ));
        let mut record = seed();
        record.wall_ns = LIMIT_NS;
        record.validate().unwrap();
        record.wall_ns += 1;
        assert!(record.validate().is_err());
        assert_eq!(CALL_SECONDS, 180);
    }
    #[test]
    fn negative_diagnostics_require_exact_stage_and_codec() {
        let first = format!(
            "marmish copy: transfer {} stream {}\n",
            "a".repeat(64),
            "b".repeat(32)
        );
        let opened =
            format!("{first}marmish copy: transport codec none\nmarmish: copy: unsafe_path\n");
        assert!(ids(opened.as_bytes(), Case::SymlinkSource, "put").is_ok());
        assert!(
            ids(
                opened.replace("codec none", "codec zlib").as_bytes(),
                Case::SymlinkSource,
                "put"
            )
            .is_err()
        );
        let early = format!("{first}marmish: copy: transfer_conflict\n");
        assert!(ids(early.as_bytes(), Case::ResumeChangedBinding, "get").is_ok());
        assert!(ids(early.as_bytes(), Case::ResumeChangedSource, "put").is_err());
        assert!(
            ids(
                format!("{opened}extra\n").as_bytes(),
                Case::SymlinkSource,
                "put"
            )
            .is_err()
        );
    }
    #[test]
    fn pre_open_put_grammar_requires_exact_refusal_and_attachment() {
        let stream = "b".repeat(32);
        let text = format!(
            "marmish copy: transfer {} stream {stream}\nmarmish copy: admission Conflict\nmarmish copy: attachment ended; remote stream {stream} may continue; use cancel --stream {stream} to request cleanup\nmarmish: copy: transfer_conflict\n",
            "a".repeat(64)
        );
        assert!(ids(text.as_bytes(), Case::ResumeChangedBinding, "put").is_ok());
        assert!(ids(text.as_bytes(), Case::ResumeChangedBinding, "get").is_err());
        assert!(
            ids(
                text.replace("admission Conflict", "admission Busy")
                    .as_bytes(),
                Case::ResumeChangedBinding,
                "put"
            )
            .is_err()
        );
    }
    fn idle_status() -> StreamStatus {
        StreamStatus {
            active: 0,
            cancelling: 0,
            uncertain: 0,
            owned_runners: 0,
            unfinished_runners: 0,
            observation_pending: false,
            cleanup_ready: true,
            stop_complete: true,
        }
    }
    fn seed() -> Observation {
        let binding = Binding {
            stream_sha256: "b".repeat(64),
            session_sha256: "c".repeat(64),
            open_digest: "d".repeat(64),
            group_sha256: "e".repeat(64),
        };
        Observation {
            schema: 1,
            case: Case::ReceiptSeed,
            kind: Case::ReceiptSeed.kind().into(),
            direction: "put".into(),
            wall_ns: 1,
            exit: 0,
            diagnostic: None,
            binding: Some(binding),
            baseline: None,
            remote_terminal: Some(marmish_stream::wire::Terminal::Exited { code: 0 }),
            transfer_sha256: "f".repeat(64),
            before_sha256: None,
            after_sha256: "a".repeat(64),
            exact_metadata: true,
            target_entry_preserved: None,
            receipt: Some(copy_checkpoint::Proof {
                transfer_sha256: "f".repeat(64),
                stream_sha256: "b".repeat(64),
                manifest_sha256: "c".repeat(64),
                phase: "committed".into(),
                durable_offset: 1,
                prefix_sha256: "a".repeat(64),
                size: 1,
                sha256: "a".repeat(64),
            }),
            client_cleanup: idle_status(),
            server_cleanup: idle_status(),
        }
    }
    #[test]
    fn receipt_schema_requires_prior_proof_and_distinct_current_stream() {
        let mut record = seed();
        record.validate().unwrap();
        record.case = Case::ResumeChangedSource;
        record.kind = record.case.kind().into();
        record.exit = 1;
        record.remote_terminal = Some(marmish_stream::wire::Terminal::Exited { code: 1 });
        record.diagnostic = record.case.diagnostic().map(str::to_owned);
        record.before_sha256 = Some(record.after_sha256.clone());
        record.baseline = record.binding.clone();
        record.binding.as_mut().unwrap().stream_sha256 = "0".repeat(64);
        record.validate().unwrap();
        let mut bad = record.clone();
        bad.baseline = None;
        assert!(bad.validate().is_err());
        let mut bad = record.clone();
        bad.receipt.as_mut().unwrap().transfer_sha256 = "0".repeat(64);
        assert!(bad.validate().is_err());
        let mut bad = record.clone();
        bad.binding = bad.baseline.clone();
        assert!(bad.validate().is_err());
        let mut bad = record.clone();
        bad.client_cleanup.unfinished_runners = 1;
        assert!(bad.validate().is_err());
        record.case = Case::ResumeChangedBinding;
        record.binding = None;
        record.remote_terminal = None;
        record.validate().unwrap();
    }
    #[test]
    fn edge_records_are_closed_and_ordinary_cannot_claim_receipts() {
        let record = seed();
        let mut value = serde_json::to_value(&record).unwrap();
        let decoded: Observation = serde_json::from_value(value.clone()).unwrap();
        decoded.validate().unwrap();
        value
            .as_object_mut()
            .unwrap()
            .insert("unexpected".into(), true.into());
        assert!(serde_json::from_value::<Observation>(value).is_err());
        let mut ordinary = record;
        ordinary.case = Case::Overwrite;
        ordinary.kind = ordinary.case.kind().into();
        assert!(ordinary.validate().is_err());
        ordinary.receipt = None;
        ordinary.validate().unwrap();
    }
    #[test]
    fn resume_args_match_actual_cli_without_transfer_id_flag() {
        let args = attempt_args(
            &"ab".repeat(32),
            &Attempt {
                direction: "put",
                source: Path::new("source"),
                destination: Path::new("destination"),
                overwrite: true,
                resume: Some("id"),
                protected: None,
            },
        )
        .unwrap();
        records::assert_public_target(&args[8], &"ab".repeat(32));
        let destination = records::public_destination(&"ab".repeat(32)).unwrap();
        assert_eq!(
            args,
            [
                "--no-autostart",
                "put",
                "--preserve-mode",
                "--preserve-mtime",
                "--overwrite",
                "--resume",
                "id",
                "--",
                &destination,
                "source",
                "destination"
            ]
            .map(OsString::from)
        );
        assert_eq!(expected_keys().len(), 22);
        assert_eq!(
            expected_keys()
                .into_iter()
                .collect::<std::collections::BTreeSet<_>>()
                .len(),
            22
        );
        assert!(expected_keys().contains(&"put-overwrite".into()));
        assert!(expected_keys().contains(&"put-receipt-seed".into()));
    }
    #[test]
    fn destination_snapshots_use_nofollow_entries_and_capture_nested_listing() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        std::fs::set_permissions(root.as_path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let sentinel = root.as_path().join("sentinel");
        payload(&sentinel, b"sentinel").unwrap();
        std::fs::set_permissions(&sentinel, std::fs::Permissions::from_mode(0o600)).unwrap();
        symlink(&sentinel, root.as_path().join("destination")).unwrap();
        let proof = destination_proof(root.as_path(), Case::DestinationSymlink).unwrap();
        assert_eq!(proof.entry.st_mode & libc::S_IFMT, libc::S_IFLNK);
        assert_eq!(proof.link.unwrap(), sentinel.as_os_str().as_bytes());
        std::fs::remove_file(root.as_path().join("destination")).unwrap();
        let target = private(root.as_path(), "destination").unwrap();
        payload(&target.join("sentinel"), b"nested").unwrap();
        std::fs::set_permissions(
            target.join("sentinel"),
            std::fs::Permissions::from_mode(0o600),
        )
        .unwrap();
        let proof = destination_proof(root.as_path(), Case::DestinationDirectory).unwrap();
        assert_eq!(proof.names.unwrap(), [b"sentinel".to_vec()]);
        assert_eq!(proof.entry.st_mode & libc::S_IFMT, libc::S_IFDIR);
    }
}

#[cfg(test)]
mod public_destination_tests {
    use super::*;
    #[test]
    fn public_destination_builder_preserves_non_utf8_copy_operands() {
        use std::os::unix::ffi::OsStringExt;
        let account = "ab".repeat(32);
        let source = std::path::PathBuf::from(OsString::from_vec(b"source \xff\n--".to_vec()));
        let destination = Path::new("destination '雪\n--");
        for direction in ["put", "get"] {
            let attempt = Attempt {
                direction,
                source: &source,
                destination,
                overwrite: true,
                resume: Some("id"),
                protected: None,
            };
            let args = attempt_args(&account, &attempt).unwrap();
            records::assert_public_target(&args[8], &account);
            assert_eq!(args[9], source.as_os_str());
            assert_eq!(args[10], destination.as_os_str());
            assert_eq!(args[5..8], ["--resume", "id", "--"]);
            assert!(attempt_args("bad", &attempt).is_err());
        }
    }
}
