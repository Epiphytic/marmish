//! Synthetic-version lifecycle qualification. No product entry point dispatches here.
use super::records::{self, Context, ROLES, TARGETS};
use crate::{Error, Result, foundation, native_manifest};
use serde::{Deserialize, Serialize};
use std::path::Path;

fn stable_version(version: &str) -> bool {
    version.len() <= 43 && !version.contains('-') && native_manifest::version(version)
}

fn bad() -> Error {
    Error::Invalid("installed_version_lifecycle")
}

/// Candidate pins remain the ordinary closed record; this wrapper adds one
/// target's genuine synthetic build and the original job watchdog boundary.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct LifecycleContext {
    schema: u8,
    kind: String,
    pub(super) candidate: Context,
    pub(super) target: String,
    pub(super) variants_sha256: String,
    pub(super) internal_deadline: u64,
}
impl LifecycleContext {
    pub(super) fn mint(
        candidate: Context,
        target: String,
        variants_sha256: String,
        internal_deadline: u64,
        now: u64,
    ) -> Result<Self> {
        candidate.validate()?;
        if internal_deadline.saturating_sub(now) < 2100 {
            return Err(bad());
        }
        candidate.require_cohort("version-lifecycle")?;
        if candidate.expires_at <= now {
            return Err(Error::Timeout);
        }
        let context = Self {
            schema: 1,
            kind: "version-lifecycle".into(),
            candidate,
            target,
            variants_sha256,
            internal_deadline,
        };
        context.validate()?;
        Ok(context)
    }
    pub(super) fn validate(&self) -> Result<()> {
        self.candidate.validate()?;
        self.candidate.require_cohort("version-lifecycle")?;
        if self.schema != 1
            || self.kind != "version-lifecycle"
            || !stable_version(&self.candidate.version)
            || !TARGETS.contains(&self.target.as_str())
            || !foundation::valid_hex(&self.variants_sha256, 64)
            || self.candidate.expires_at - self.candidate.created_at != 1800
            || self.candidate.expires_at > self.internal_deadline.saturating_sub(300)
        {
            return Err(bad());
        }
        Ok(())
    }
    pub(super) fn digest(&self) -> Result<String> {
        self.validate()?;
        Ok(records::digest(&records::canonical(self)?))
    }
    pub(super) fn role(&self) -> Result<&'static str> {
        TARGETS
            .iter()
            .position(|target| *target == self.target)
            .map(|index| ROLES[index])
            .ok_or_else(bad)
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Variant {
    pub(super) version: String,
    pub(super) commit: String,
    pub(super) tree: String,
    pub(super) patch_sha256: String,
    pub(super) manifest_sha256: String,
    pub(super) signing_kind: String,
    pub(super) build_ms: u64,
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Variants {
    schema: u8,
    pub(super) source_commit: String,
    pub(super) target: String,
    pub(super) b_manifest_sha256: String,
    pub(super) a: Variant,
    pub(super) c: Variant,
}
impl Variants {
    fn validate(&self, ctx: &LifecycleContext) -> Result<()> {
        if self.schema != 1
            || self.source_commit != ctx.candidate.source_commit
            || self.target != ctx.target
            || self.b_manifest_sha256
                != *ctx.candidate.manifests.get(ctx.role()?).ok_or_else(bad)?
            || self.a.version != format!("{}-rc.1", ctx.candidate.version)
            || self.c.version != format!("{}-rc.2", ctx.candidate.version)
            || !stable_version(&ctx.candidate.version)
            || self.a.commit == self.c.commit
            || self.a.tree == self.c.tree
        {
            return Err(bad());
        }
        for variant in [&self.a, &self.c] {
            if !foundation::valid_hex(&variant.commit, 40)
                || !foundation::valid_hex(&variant.tree, 40)
                || !foundation::valid_hex(&variant.patch_sha256, 64)
                || !foundation::valid_hex(&variant.manifest_sha256, 64)
                || variant.commit == self.source_commit
                || variant.build_ms == 0
                || variant.build_ms > 2_700_000
                || variant.signing_kind
                    != if self.target == "aarch64-apple-darwin" {
                        "adhoc"
                    } else {
                        "none"
                    }
            {
                return Err(bad());
            }
        }
        Ok(())
    }
    /// Verify real bytes through the product manifest verifier. A same-byte
    /// relabel cannot satisfy the manifest's embedded version/commit probes in
    /// the subsequent installed metadata stage.
    pub(super) fn load(
        root: &Path,
        candidate_assets: &Path,
        ctx: &LifecycleContext,
    ) -> Result<Self> {
        ctx.validate()?;
        let bytes = records::read_bytes(&root.join("variants.json"), 16384)?;
        if records::digest(&bytes) != ctx.variants_sha256 {
            return Err(bad());
        }
        let variants: Self = serde_json::from_slice(&bytes).map_err(|_| bad())?;
        if records::canonical(&variants)? != bytes {
            return Err(bad());
        }
        variants.validate(ctx)?;
        let candidate = native_manifest::verify_directory(
            candidate_assets,
            native_manifest::AssetNaming::VersionedDownloads,
            native_manifest::hex32(&variants.b_manifest_sha256)?,
        )?;
        if candidate.target != ctx.target
            || candidate.commit != ctx.candidate.source_commit
            || candidate.version != ctx.candidate.version
            || candidate.formats[1] <= 4
        {
            return Err(bad());
        }
        for (label, variant) in [("a", &variants.a), ("c", &variants.c)] {
            let manifest = native_manifest::verify_directory(
                &root.join(label),
                native_manifest::AssetNaming::VersionedDownloads,
                native_manifest::hex32(&variant.manifest_sha256)?,
            )?;
            let mut formats = candidate.formats;
            if label == "c" {
                formats[1] = 4;
            }
            if manifest.target != ctx.target
                || manifest.version != variant.version
                || manifest.commit != variant.commit
                || manifest.signing != variant.signing_kind
                || manifest.formats != formats
                || records::digest(&records::read_bytes(
                    &root.join("patches").join(format!("{label}.patch")),
                    16384,
                )?) != variant.patch_sha256
            {
                return Err(bad());
            }
        }
        Ok(variants)
    }
}

/// Public records describe only finite proof outcomes, never private paths,
/// provider errors, identity material, or captured product output.
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Stage {
    Inputs,
    InstallA,
    RunningA,
    UpgradeB,
    RecoverB,
    RollbackA,
    RejectC,
    Provider,
    Cleanup,
}
const STAGES: [Stage; 9] = [
    Stage::Inputs,
    Stage::InstallA,
    Stage::RunningA,
    Stage::UpgradeB,
    Stage::RecoverB,
    Stage::RollbackA,
    Stage::RejectC,
    Stage::Provider,
    Stage::Cleanup,
];
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Failure {
    Inputs,
    Timeout,
    Interrupted,
    Runtime,
    Cleanup,
    Record,
    Provider(super::lifecycle_provider::ProviderFailure),
}
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Disposition {
    Passed,
    Failed,
    Unreached,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StageResult {
    stage: Stage,
    disposition: Disposition,
    #[serde(deserialize_with = "super::cleanup::required_nullable")]
    elapsed_ms: Option<u64>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct LifecycleResult {
    synthetic: bool,
    schema: u8,
    kind: String,
    context_sha256: String,
    source_commit: String,
    role: String,
    target: String,
    variants_sha256: String,
    route: String,
    stages: Vec<StageResult>,
    #[serde(deserialize_with = "super::cleanup::required_nullable")]
    failure: Option<Failure>,
    #[serde(deserialize_with = "super::cleanup::required_nullable")]
    command_wall_ms: Option<u64>,
    #[serde(deserialize_with = "super::cleanup::required_nullable")]
    restart_ready_ms: Option<u64>,
    #[serde(deserialize_with = "super::cleanup::required_nullable")]
    stdout_bytes: Option<u64>,
    #[serde(deserialize_with = "super::cleanup::required_nullable")]
    stderr_bytes: Option<u64>,
    worker_inode_retained: bool,
    helper_inode_retained: bool,
    single_execution: bool,
    original_submit_existing: bool,
    finish_confirmed: bool,
    positive_rollback_loaded: bool,
    incompatible_start_refused: bool,
    database_inventory_unchanged: bool,
    restored_b_loaded: bool,
    provider_required: bool,
    provider_upgrade_loaded: bool,
    provider_rollback_loaded: bool,
    provider_locked_denied: bool,
    engine_cleanup_complete: bool,
    server_stop_complete: bool,
    client_stop_complete: bool,
    provider_cleanup_complete: bool,
    workspace_removed: bool,
    complete: bool,
}
impl LifecycleResult {
    fn new(ctx: &LifecycleContext) -> Result<Self> {
        Ok(Self {
            schema: 1,
            kind: "version-lifecycle-result".into(),
            synthetic: true,
            context_sha256: ctx.digest()?,
            source_commit: ctx.candidate.source_commit.clone(),
            role: ctx.role()?.into(),
            target: ctx.target.clone(),
            variants_sha256: ctx.variants_sha256.clone(),
            route: "direct".into(),
            stages: STAGES
                .into_iter()
                .map(|stage| StageResult {
                    stage,
                    disposition: Disposition::Unreached,
                    elapsed_ms: None,
                })
                .collect(),
            failure: None,
            command_wall_ms: None,
            restart_ready_ms: None,
            stdout_bytes: None,
            stderr_bytes: None,
            worker_inode_retained: false,
            helper_inode_retained: false,
            single_execution: false,
            original_submit_existing: false,
            finish_confirmed: false,
            positive_rollback_loaded: false,
            incompatible_start_refused: false,
            database_inventory_unchanged: false,
            restored_b_loaded: false,
            provider_required: ctx.target == "aarch64-apple-darwin",
            provider_upgrade_loaded: false,
            provider_rollback_loaded: false,
            provider_locked_denied: false,
            engine_cleanup_complete: false,
            server_stop_complete: false,
            client_stop_complete: false,
            provider_cleanup_complete: false,
            workspace_removed: false,
            complete: false,
        })
    }
    pub(super) fn is_complete(&self) -> bool {
        self.complete
    }
}
pub(super) fn validate_record(r: &LifecycleResult, ctx: &LifecycleContext) -> Result<()> {
    ctx.validate()?;
    if !r.synthetic
        || r.schema != 1
        || r.kind != "version-lifecycle-result"
        || r.context_sha256 != ctx.digest()?
        || r.source_commit != ctx.candidate.source_commit
        || r.role != ctx.role()?
        || r.target != ctx.target
        || r.variants_sha256 != ctx.variants_sha256
        || r.route != "direct"
        || r.provider_required != (ctx.target == "aarch64-apple-darwin")
        || r.stages.len() != STAGES.len()
        || r.command_wall_ms.is_some_and(|n| n == 0 || n > 900_000)
        || r.restart_ready_ms.is_some_and(|n| n == 0 || n > 150_000)
        || r.stdout_bytes.is_some_and(|n| n != 131072)
        || r.stderr_bytes.is_some_and(|n| n != 131072)
    {
        return Err(bad());
    }
    let mut unfinished = false;
    for (entry, stage) in r.stages.iter().zip(STAGES) {
        if entry.stage != stage
            || entry.elapsed_ms.is_some_and(|n| n > 1_800_000)
            || (entry.disposition == Disposition::Unreached) != entry.elapsed_ms.is_none()
            || unfinished && entry.disposition != Disposition::Unreached && stage != Stage::Cleanup
        {
            return Err(bad());
        }
        unfinished |= entry.disposition != Disposition::Passed;
    }
    for (proof, stage) in [
        (
            r.worker_inode_retained || r.helper_inode_retained,
            Stage::UpgradeB,
        ),
        (
            r.single_execution
                || r.original_submit_existing
                || r.finish_confirmed
                || r.command_wall_ms.is_some()
                || r.restart_ready_ms.is_some()
                || r.stdout_bytes.is_some()
                || r.stderr_bytes.is_some(),
            Stage::RecoverB,
        ),
        (r.positive_rollback_loaded, Stage::RollbackA),
        (
            r.incompatible_start_refused || r.database_inventory_unchanged || r.restored_b_loaded,
            Stage::RejectC,
        ),
        (
            r.provider_upgrade_loaded || r.provider_rollback_loaded || r.provider_locked_denied,
            Stage::Provider,
        ),
    ] {
        if proof
            && !r
                .stages
                .iter()
                .any(|s| s.stage == stage && s.disposition != Disposition::Unreached)
        {
            return Err(bad());
        }
    }
    if r.complete && r.failure.is_some() {
        return Err(bad());
    }
    if !r.complete
        && r.stages
            .iter()
            .any(|s| s.disposition == Disposition::Failed)
        && r.failure.is_none()
    {
        return Err(bad());
    }
    if r.complete
        && (unfinished
            || !r.worker_inode_retained
            || !r.helper_inode_retained
            || !r.single_execution
            || !r.original_submit_existing
            || !r.finish_confirmed
            || !r.positive_rollback_loaded
            || !r.incompatible_start_refused
            || !r.database_inventory_unchanged
            || !r.restored_b_loaded
            || r.command_wall_ms.is_none()
            || r.restart_ready_ms.is_none()
            || r.stdout_bytes != Some(131072)
            || r.stderr_bytes != Some(131072)
            || !r.engine_cleanup_complete
            || !r.server_stop_complete
            || !r.client_stop_complete
            || !r.provider_cleanup_complete
            || !r.workspace_removed
            || r.provider_required
                && (!r.provider_upgrade_loaded
                    || !r.provider_rollback_loaded
                    || !r.provider_locked_denied))
    {
        return Err(bad());
    }
    Ok(())
}
pub(super) fn validate(r: &LifecycleResult, ctx: &LifecycleContext) -> Result<()> {
    validate_record(r, ctx)?;
    if !r.complete {
        return Err(bad());
    }
    Ok(())
}
pub(super) fn markdown_summary(r: &LifecycleResult) -> String {
    format!(
        "{}: {} (failure: {})\n",
        r.role,
        if r.complete { "passed" } else { "failed" },
        serde_json::to_string(&r.failure).unwrap_or_else(|_| "null".into())
    )
}

pub(super) struct RollupPins<'a> {
    pub(super) source: &'a str,
    pub(super) run_id: u64,
    pub(super) attempt: u64,
    pub(super) harness_sha256: &'a str,
    pub(super) candidate_run_id: u64,
    pub(super) candidate_attempt: u64,
}
pub(super) fn rollup(
    input: &Path,
    output: &Path,
    job_result: &str,
    pins: RollupPins<'_>,
) -> Result<()> {
    if !matches!(job_result, "success" | "failure" | "cancelled" | "skipped") {
        return Err(bad());
    }
    crate::identity::check_dir(input)?;
    let expected: std::collections::BTreeSet<_> = ROLES
        .into_iter()
        .flat_map(|role| {
            [
                format!("{role}-context.json"),
                format!("{role}-result.json"),
            ]
        })
        .collect();
    let actual = std::fs::read_dir(input)?
        .map(|entry| entry?.file_name().into_string().map_err(|_| bad()))
        .collect::<Result<std::collections::BTreeSet<_>>>()?;
    if actual != expected {
        return Err(bad());
    }
    let mut reports = Vec::new();
    let mut reference: Option<Context> = None;
    let mut complete = job_result == "success";
    let mut markdown = String::from(
        "# Installed synthetic version lifecycle\n\nA/C are synthetic mechanism fixtures, not historical releases.\n\n",
    );
    for role in ROLES {
        let ctx: LifecycleContext =
            records::read(&input.join(format!("{role}-context.json")), 65536)?;
        let report: LifecycleResult =
            records::read(&input.join(format!("{role}-result.json")), 65536)?;
        validate_record(&report, &ctx)?;
        if ctx.role()? != role
            || ctx.candidate.source_commit != pins.source
            || ctx.candidate.run_id != pins.run_id
            || ctx.candidate.run_attempt != pins.attempt
            || ctx.candidate.harness_sha256 != pins.harness_sha256
            || ctx.candidate.candidate_run_id != pins.candidate_run_id
            || ctx.candidate.candidate_run_attempt != pins.candidate_attempt
        {
            return Err(bad());
        }
        if let Some(previous) = &reference {
            let mut left = previous.clone();
            let mut right = ctx.candidate.clone();
            // Contexts are minted only after each target's real build completes;
            // pins and run identity must match while target clocks/nonces differ.
            left.created_at = 0;
            right.created_at = 0;
            left.expires_at = 1800;
            right.expires_at = 1800;
            left.nonce.clear();
            right.nonce.clear();
            if records::canonical(&left)? != records::canonical(&right)? {
                return Err(bad());
            }
        } else {
            reference = Some(ctx.candidate.clone());
        }
        complete &= report.is_complete();
        markdown.push_str(&markdown_summary(&report));
        reports.push(report);
    }
    #[derive(Serialize)]
    struct Rollup<'a> {
        schema: u8,
        kind: &'static str,
        job_result: &'a str,
        complete: bool,
        records: &'a [LifecycleResult],
    }
    records::write(
        &output.join("lifecycle-rollup.json"),
        &Rollup {
            schema: 1,
            kind: "version-lifecycle-rollup",
            job_result,
            complete,
            records: &reports,
        },
    )?;
    records::write_raw(&output.join("lifecycle-summary.md"), markdown.as_bytes())?;
    if !complete {
        return Err(bad());
    }
    Ok(())
}

/// Retain open descriptors to the exact old product files. Merely observing the
/// same path after a selector change does not prove that its inode survived.
struct RetainedFile {
    file: std::fs::File,
    dev: u64,
    ino: u64,
    bytes: u64,
    sha256: String,
}
impl RetainedFile {
    fn capture(path: &Path) -> Result<Self> {
        use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
        let file = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
            .open(path)?;
        let m = file.metadata()?;
        if !m.is_file()
            || m.uid() != unsafe { libc::geteuid() }
            || m.mode() & 0o022 != 0
            || m.len() == 0
            || m.len() > 256 * 1024 * 1024
        {
            return Err(bad());
        }
        let sha256 = records::digest(&native_manifest::read_bounded(path, 256 * 1024 * 1024)?);
        Ok(Self {
            file,
            dev: m.dev(),
            ino: m.ino(),
            bytes: m.len(),
            sha256,
        })
    }
    fn verify(&self, path: &Path) -> Result<()> {
        use std::os::unix::fs::MetadataExt;
        let m = std::fs::symlink_metadata(path)?;
        let held = self.file.metadata()?;
        if !m.is_file()
            || (m.dev(), m.ino(), m.len()) != (self.dev, self.ino, self.bytes)
            || (held.dev(), held.ino(), held.len()) != (self.dev, self.ino, self.bytes)
            || records::digest(&native_manifest::read_bounded(path, 256 * 1024 * 1024)?)
                != self.sha256
        {
            return Err(bad());
        }
        Ok(())
    }
}
/// Main databases and every preexisting auxiliary file remain byte-identical.
struct DatabaseInventory {
    databases: Vec<crate::native_preflight::WalSnapshot>,
    root: std::path::PathBuf,
    files: std::collections::BTreeMap<std::path::PathBuf, (u64, String)>,
}
impl DatabaseInventory {
    fn files(root: &Path) -> Result<std::collections::BTreeMap<std::path::PathBuf, (u64, String)>> {
        use sha2::{Digest as _, Sha256};
        use std::io::Read;
        let mut pending = vec![root.to_path_buf()];
        let mut files = std::collections::BTreeMap::new();
        while let Some(dir) = pending.pop() {
            crate::identity::check_dir(&dir)?;
            for entry in std::fs::read_dir(&dir)? {
                let path = entry?.path();
                let meta = std::fs::symlink_metadata(&path)?;
                if meta.is_dir() {
                    pending.push(path);
                    continue;
                }
                let mut file = crate::identity::open_private(&path, false)?;
                if meta.len() > 64 * 1024 * 1024 || files.len() >= 4096 {
                    return Err(bad());
                }
                let mut digest = Sha256::new();
                let mut block = [0u8; 8192];
                let mut read = 0u64;
                loop {
                    let n = file.read(&mut block)?;
                    read = read.checked_add(n as u64).ok_or_else(bad)?;
                    if read > 64 * 1024 * 1024 {
                        return Err(bad());
                    }
                    if n == 0 {
                        break;
                    }
                    digest.update(&block[..n]);
                }
                if read != meta.len() {
                    return Err(bad());
                }
                files.insert(path, (meta.len(), hex::encode(digest.finalize())));
            }
        }
        Ok(files)
    }
    fn capture(profile: &Path) -> Result<Self> {
        Ok(Self {
            databases: ["mdk.db", "journal.db", "commands/engine.sqlite3"]
                .into_iter()
                .map(|name| crate::native_preflight::WalSnapshot::capture(&profile.join(name)))
                .collect::<Result<Vec<_>>>()?,
            root: profile.into(),
            files: Self::files(profile)?,
        })
    }
    fn verify(&self) -> Result<()> {
        crate::native_preflight::WalSnapshot::verify_all(&self.databases)?;
        let mut actual = Self::files(&self.root)?;
        for base in ["mdk.db", "journal.db", "commands/engine.sqlite3"] {
            for suffix in ["-wal", "-shm"] {
                let p = self.root.join(format!("{base}{suffix}"));
                if !self.files.contains_key(&p) {
                    actual.remove(&p);
                }
            }
        }
        if actual != self.files {
            return Err(bad());
        }
        Ok(())
    }
}

fn directory(path: &Path) -> Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    std::fs::DirBuilder::new().mode(0o700).create(path)?;
    crate::identity::check_dir(path)
}
pub(super) struct Cancellation(tokio::sync::watch::Receiver<bool>);
impl Cancellation {
    #[cfg(test)]
    pub(super) fn channel() -> (tokio::sync::watch::Sender<bool>, Self) {
        let (sender, receiver) = tokio::sync::watch::channel(false);
        (sender, Self(receiver))
    }
    pub(super) fn check(&self) -> Result<()> {
        if *self.0.borrow() {
            Err(Error::Invalid("lifecycle_interrupted"))
        } else {
            Ok(())
        }
    }
    pub(super) async fn cancelled(&self) {
        let mut rx = self.0.clone();
        if *rx.borrow() {
            return;
        }
        let _ = rx.changed().await;
    }
}
async fn execute(
    executable: &Path,
    args: &[std::ffi::OsString],
    cwd: &Path,
    env: &super::process::Environment,
    input: Vec<u8>,
    deadline: tokio::time::Instant,
    cancel: &Cancellation,
) -> Result<super::process::Output> {
    use tokio::io::AsyncWriteExt;
    if *cancel.0.borrow() || tokio::time::Instant::now() >= deadline {
        return Err(Error::Timeout);
    }
    let mut cmd = super::process::command(executable, args, cwd, env);
    cmd.stdin(std::process::Stdio::piped());
    let mut child = cmd.spawn()?;
    let mut stdin = child.stdin.take().ok_or_else(bad)?;
    let mut writer = tokio::spawn(async move {
        stdin.write_all(&input).await?;
        stdin.shutdown().await
    });
    let mut out = tokio::spawn(super::process::drain(
        child.stdout.take().ok_or_else(bad)?,
        524288,
    ));
    let mut err = tokio::spawn(super::process::drain(
        child.stderr.take().ok_or_else(bad)?,
        524288,
    ));
    let status = tokio::select! {biased;_=cancel.cancelled()=>Err(Error::Invalid("lifecycle_interrupted")),_=tokio::time::sleep_until(deadline)=>Err(Error::Timeout),s=child.wait()=>s.map_err(Error::from)};
    let result = match status {
        Ok(status) => tokio::time::timeout_at(deadline, async {
            (&mut writer).await.map_err(|_| bad())??;
            let stdout = (&mut out).await.map_err(|_| bad())??;
            let stderr = (&mut err).await.map_err(|_| bad())??;
            if stdout.1 || stderr.1 {
                return Err(bad());
            }
            Ok(super::process::Output {
                status,
                stdout: stdout.0,
                stderr: stderr.0,
            })
        })
        .await
        .map_err(|_| Error::Timeout)
        .and_then(|r| r),
        Err(e) => {
            let _ = super::process::terminate(&mut child).await;
            Err(e)
        }
    };
    if result.is_err() {
        writer.abort();
        out.abort();
        err.abort();
        if !writer.is_finished() {
            let _ = writer.await;
        }
        if !out.is_finished() {
            let _ = out.await;
        }
        if !err.is_finished() {
            let _ = err.await;
        }
    }
    result
}
struct Inputs<'a> {
    cancel: &'a Cancellation,
    ctx: &'a LifecycleContext,
    variants: &'a Variants,
    candidate_assets: &'a Path,
    variant_assets: &'a Path,
    bootstrap: &'a Path,
}
struct Installation {
    manifest: native_manifest::Manifest,
    version_root: std::path::PathBuf,
}
impl Inputs<'_> {
    async fn install(
        &self,
        workspace: &Path,
        environment: &super::process::Environment,
        variant: Option<&Variant>,
        deadline: tokio::time::Instant,
    ) -> Result<Installation> {
        use std::ffi::OsString;
        let (assets, manifest_pin) = match variant {
            Some(v) => (
                self.variant_assets
                    .join(if v.version == self.variants.a.version {
                        "a"
                    } else {
                        "c"
                    }),
                v.manifest_sha256.as_str(),
            ),
            None => (
                self.candidate_assets.to_path_buf(),
                self.variants.b_manifest_sha256.as_str(),
            ),
        };
        let manifest = native_manifest::verify_directory(
            &assets,
            native_manifest::AssetNaming::VersionedDownloads,
            native_manifest::hex32(manifest_pin)?,
        )?;
        let mut args: Vec<OsString>;
        let launcher;
        if variant.is_some() {
            launcher = assets.join(format!("marmish-{}-{}", manifest.version, manifest.target));
            let manifest_path = assets.join(format!(
                "marmish-{}-{}.manifest",
                manifest.version, manifest.target
            ));
            args = vec![
                "install".into(),
                "--source-dir".into(),
                assets.into_os_string(),
                "--manifest".into(),
                manifest_path.into_os_string(),
                "--manifest-sha256".into(),
                manifest_pin.into(),
            ];
        } else {
            launcher = std::path::PathBuf::from("/bin/sh");
            args = vec![
                self.bootstrap.as_os_str().into(),
                "--asset-dir".into(),
                assets.into_os_string(),
            ];
        }
        args.extend([
            "--prefix".into(),
            workspace.join("install").into_os_string(),
            "--bin-dir".into(),
            workspace.join("bin").into_os_string(),
            "--state-dir".into(),
            workspace.join("profiles").into_os_string(),
        ]);
        if variant.is_none() {
            args.extend(["--key-storage".into(), "file".into()]);
        }
        let output = execute(
            &launcher,
            &args,
            workspace,
            environment,
            Vec::new(),
            deadline,
            self.cancel,
        )
        .await?;
        if !output.status.success() {
            return Err(bad());
        }
        let executable = workspace.join("bin/marmish").canonicalize()?;
        let version_root = executable.parent().ok_or_else(bad)?.to_path_buf();
        if version_root != workspace.join("install/versions").join(manifest.id()) {
            return Err(bad());
        }
        let installed = native_manifest::verify_directory(
            &version_root,
            native_manifest::AssetNaming::InstalledFixedNames,
            native_manifest::hex32(manifest_pin)?,
        )?;
        if installed != manifest {
            return Err(bad());
        }
        Ok(Installation {
            manifest,
            version_root,
        })
    }
    async fn setup(
        &self,
        workspace: &Path,
        environment: &super::process::Environment,
        installation: &Installation,
        deadline: tokio::time::Instant,
    ) -> Result<records::IdentityReady> {
        use std::ffi::OsString;
        let mut accounts = Vec::new();
        for name in ["client", "server"] {
            let mut args: Vec<OsString> = vec![
                "--state-dir".into(),
                workspace.join("profiles").into_os_string(),
                "--identity".into(),
                name.into(),
                "--key-storage".into(),
                "file".into(),
                "--tor".into(),
                "off".into(),
            ];
            for relay in &self.ctx.candidate.relays {
                args.extend(["--relay".into(), relay.into()]);
            }
            args.extend([
                "setup".into(),
                "--role".into(),
                name.into(),
                "--no-grants".into(),
            ]);
            let output = execute(
                &workspace.join("bin/marmish"),
                &args,
                workspace,
                environment,
                Vec::new(),
                deadline,
                self.cancel,
            )
            .await?;
            if !output.status.success() {
                return Err(bad());
            }
            let setup: super::prepare::Setup =
                serde_json::from_slice(&output.stdout).map_err(|_| bad())?;
            if setup.schema != 1
                || setup.identity != name
                || setup.backend != "file"
                || setup.role != name
                || setup.inbound_enabled
                || !setup.config_changed
                || !setup.identity_created
                || !setup.owner_opened
                || !setup.engine_stop_complete
                || !setup.mdk_stop_complete
                || !setup.runtime_cleanup_complete
            {
                return Err(bad());
            }
            accounts.push(hex::encode(crate::command_config::account(&setup.npub)?));
            super::cleanup::require_no_engine(&workspace.join("profiles").join(name))?;
        }
        if accounts[0] == accounts[1] {
            return Err(bad());
        }
        // This internal identity describes the actual A (or B) installation. It
        // is never published as a candidate-B identity-ready record.
        Ok(records::IdentityReady {
            schema: 1,
            kind: "identity-ready".into(),
            context_sha256: self.ctx.digest()?,
            role: self.ctx.role()?.into(),
            nonce: records::digest(&records::canonical(&(
                self.ctx.digest()?,
                "lifecycle-identity",
            ))?),
            client_account: accounts[0].clone(),
            server_account: accounts[1].clone(),
            installation: records::Installation {
                version: installation.manifest.version.clone(),
                target: installation.manifest.target.clone(),
                source_commit: installation.manifest.commit.clone(),
                manifest_sha256: if installation.manifest.version == self.variants.a.version {
                    self.variants.a.manifest_sha256.clone()
                } else {
                    self.variants.b_manifest_sha256.clone()
                },
                bootstrap_sha256: self.ctx.candidate.bootstrap_sha256.clone(),
                files: installation
                    .manifest
                    .files
                    .iter()
                    .map(|asset| {
                        (
                            asset.name.to_owned(),
                            records::FilePin {
                                sha256: hex::encode(asset.sha256),
                                bytes: asset.bytes,
                            },
                        )
                    })
                    .collect(),
            },
            identity_ready_at: super::now()?,
        })
    }
    async fn select(
        &self,
        workspace: &Path,
        environment: &super::process::Environment,
        id: &str,
        deadline: tokio::time::Instant,
    ) -> Result<()> {
        let output = execute(
            &workspace.join("bin/marmish"),
            &[
                "install".into(),
                "--select".into(),
                id.into(),
                "--prefix".into(),
                workspace.join("install").into_os_string(),
                "--bin-dir".into(),
                workspace.join("bin").into_os_string(),
                "--state-dir".into(),
                workspace.join("profiles").into_os_string(),
            ],
            workspace,
            environment,
            Vec::new(),
            deadline,
            self.cancel,
        )
        .await?;
        if !output.status.success()
            || workspace.join("bin/marmish").canonicalize()?
                != workspace.join("install/versions").join(id).join("marmish")
        {
            return Err(bad());
        }
        Ok(())
    }
}

struct Command {
    nonce: String,
    handle: crate::command_client::CommandHandle,
    handle_file: std::path::PathBuf,
    group_submit: Option<String>,
    began: tokio::time::Instant,
}
struct Pair {
    server: Option<super::matrix::Daemon>,
    client: Option<super::matrix::Daemon>,
    tree: Option<super::super::command_check::EngineTree>,
    client_receipt: super::cleanup::ProfileReceipt,
    sockets: std::path::PathBuf,
    launched: tokio::time::Instant,
    expected: std::collections::BTreeMap<(String, String), (String, String)>,
    unknown: bool,
}
impl Pair {
    async fn start(
        w: &Path,
        ctx: &LifecycleContext,
        env: &super::process::Environment,
        deadline: tokio::time::Instant,
    ) -> Result<Self> {
        use std::os::unix::fs::DirBuilderExt;
        super::cleanup::require_no_engine(&w.join("profiles/client"))?;
        let receipt = super::cleanup::ProfileReceipt::capture(&w.join("profiles/client"))?;
        let mut random = [0; 8];
        getrandom::fill(&mut random).map_err(|_| Error::Identity("random"))?;
        let sockets = std::env::temp_dir()
            .canonicalize()?
            .join(format!("ml-{}", hex::encode(random)));
        std::fs::DirBuilder::new().mode(0o700).create(&sockets)?;
        let launched = tokio::time::Instant::now();
        let server = super::matrix::Daemon::start(
            w,
            "server",
            sockets.join("s"),
            env,
            &ctx.candidate.relays,
            deadline
                .saturating_duration_since(tokio::time::Instant::now())
                .as_secs()
                .min(1800),
        )?;
        let client = match super::matrix::Daemon::start(
            w,
            "client",
            sockets.join("c"),
            env,
            &ctx.candidate.relays,
            deadline
                .saturating_duration_since(tokio::time::Instant::now())
                .as_secs()
                .min(1800),
        ) {
            Ok(d) => d,
            Err(e) => {
                let _ = server.ordinary_stop(deadline).await;
                let _ = std::fs::remove_dir(&sockets);
                return Err(e);
            }
        };
        Ok(Self {
            server: Some(server),
            client: Some(client),
            tree: None,
            client_receipt: receipt,
            sockets,
            launched,
            expected: Default::default(),
            unknown: false,
        })
    }
    async fn ready(&mut self, w: &Path, deadline: tokio::time::Instant) -> Result<()> {
        let server = self.server.as_mut().ok_or_else(bad)?;
        let client = self.client.as_mut().ok_or_else(bad)?;
        tokio::try_join!(
            super::matrix::ready(server, deadline),
            super::matrix::ready(client, deadline)
        )?;
        self.client_receipt.verify(&w.join("profiles/client"))?;
        super::cleanup::verify_no_engine(
            &mut client.child,
            &w.join("profiles/client"),
            &client.socket,
            deadline,
        )
        .await?;
        Ok(())
    }
    async fn stop_ordinary(&mut self, w: &Path, deadline: tokio::time::Instant) -> Result<()> {
        self.client_receipt.verify(&w.join("profiles/client"))?;
        let client_ref = self.client.as_mut().ok_or_else(bad)?;
        super::cleanup::verify_no_engine(
            &mut client_ref.child,
            &w.join("profiles/client"),
            &client_ref.socket,
            deadline,
        )
        .await?;
        if let Some(tree) = &self.tree {
            tree.verify_path(&w.join("profiles/server"))?;
        }
        let client = self.client.take().ok_or_else(bad)?;
        let server = self.server.take().ok_or_else(bad)?;
        let (s, c) = tokio::join!(
            server.ordinary_stop(deadline),
            client.stop_no_engine(deadline)
        );
        s?;
        c?;
        self.client_receipt.verify(&w.join("profiles/client"))?;
        super::cleanup::require_no_engine(&w.join("profiles/client"))?;
        if let Some(tree) = &self.tree {
            tree.verify_path(&w.join("profiles/server"))?;
        }
        Ok(())
    }
    async fn restart(
        &mut self,
        w: &Path,
        ctx: &LifecycleContext,
        env: &super::process::Environment,
        deadline: tokio::time::Instant,
    ) -> Result<()> {
        if let Some(tree) = &self.tree {
            tree.verify_path(&w.join("profiles/server"))?;
        }
        let seconds = (self.launched + std::time::Duration::from_secs(1800))
            .min(deadline)
            .saturating_duration_since(tokio::time::Instant::now())
            .as_secs();
        if seconds < 300 {
            return Err(Error::Timeout);
        }
        self.client_receipt.verify(&w.join("profiles/client"))?;
        self.server = Some(super::matrix::Daemon::start(
            w,
            "server",
            self.sockets.join("s"),
            env,
            &ctx.candidate.relays,
            seconds,
        )?);
        self.client = Some(super::matrix::Daemon::start(
            w,
            "client",
            self.sockets.join("c"),
            env,
            &ctx.candidate.relays,
            seconds,
        )?);
        self.ready(w, deadline).await
    }
    async fn cleanup(
        &mut self,
        w: &Path,
        deadline: tokio::time::Instant,
        failed: bool,
    ) -> (bool, bool, bool) {
        let work = deadline
            .checked_sub(std::time::Duration::from_secs(60))
            .unwrap_or(deadline);
        let mut engine = false;
        if cleanup_allowed(self.unknown, self.expected.len())
            && let Some(server) = self.server.as_mut()
        {
            if failed {
                let _ = super::cleanup::cancel_fixture(
                    &mut server.child,
                    &server.socket,
                    &self.expected,
                    2,
                    work,
                )
                .await;
            }
            if let Some(tree) = self.tree.take()
                && let Ok(mut guard) = super::cleanup::prepare(
                    &mut server.child,
                    &w.join("profiles/server"),
                    &server.socket,
                    self.expected.clone(),
                    work,
                )
                .await
            {
                engine = tree.remove_installed(&mut guard).await.is_ok();
            }
        }
        let server = self.server.take();
        let client = self.client.take();
        let (server_ok, client_ok) = tokio::join!(
            async {
                match server {
                    Some(d) => d.stop(deadline).await.is_ok(),
                    None => false,
                }
            },
            async {
                let Some(mut d) = client else {
                    return false;
                };
                let path = w.join("profiles/client");
                let proof = self.client_receipt.verify(&path).is_ok()
                    && super::cleanup::verify_no_engine(&mut d.child, &path, &d.socket, work)
                        .await
                        .is_ok();
                let stop = d.stop_no_engine(deadline).await;
                proof
                    && stop.is_ok()
                    && self.client_receipt.verify(&path).is_ok()
                    && super::cleanup::require_no_engine(&path).is_ok()
            }
        );
        if server_ok && client_ok {
            let _ = std::fs::remove_dir(&self.sockets);
        }
        (engine, server_ok, client_ok)
    }
}
struct CommandClient<'a> {
    cancel: &'a Cancellation,
    workspace: &'a Path,
    ctx: &'a LifecycleContext,
    identity: &'a records::IdentityReady,
    environment: &'a super::process::Environment,
    executable: &'a Path,
    deadline: tokio::time::Instant,
}
impl CommandClient<'_> {
    async fn call(
        &self,
        args: &[std::ffi::OsString],
        input: Vec<u8>,
    ) -> Result<super::process::Output> {
        execute(
            &self.workspace.join("bin/marmish"),
            args,
            self.workspace,
            self.environment,
            input,
            self.deadline,
            self.cancel,
        )
        .await
    }
    fn selected(&self, pair: &Pair, name: &str, handle: &Path) -> Result<Vec<std::ffi::OsString>> {
        let socket = &pair.client.as_ref().ok_or_else(bad)?.socket;
        let mut args = super::matrix::base(self.workspace, "client", socket, &[]);
        args.extend([
            "--no-autostart".into(),
            "--wait-seconds".into(),
            "300".into(),
            name.into(),
            "--handle-file".into(),
            handle.as_os_str().into(),
        ]);
        Ok(args)
    }
    async fn submit(&self, pair: &mut Pair) -> Result<Command> {
        use nostr::nips::nip19::ToBech32;
        let nonce = records::digest(&records::canonical(&(
            "version-lifecycle/1",
            self.ctx.digest()?,
            &self.identity.client_account,
        ))?);
        let socket = &pair.client.as_ref().ok_or_else(bad)?.socket;
        let mut args =
            super::matrix::base(self.workspace, "client", socket, &self.ctx.candidate.relays);
        args.extend([
            "--no-autostart".into(),
            "--stdin".into(),
            "--detach".into(),
            "--wait-seconds".into(),
            "90".into(),
            nostr::key::PublicKey::from_hex(&self.identity.server_account)
                .map_err(|_| bad())?
                .to_bech32()
                .map_err(|_| bad())?
                .into(),
            "--".into(),
            self.executable.as_os_str().into(),
        ]);
        args.extend(super::fixture::recovery_arguments(&nonce)?);
        pair.unknown = true;
        let began = tokio::time::Instant::now();
        let output = self
            .call(&args, super::fixture::bytes(&nonce, 0)?.0)
            .await?;
        if !output.status.success() || !output.stdout.is_empty() {
            return Err(bad());
        }
        let bytes = output
            .stderr
            .strip_prefix(b"marmish handle: ")
            .and_then(|b| b.strip_suffix(b"\n"))
            .ok_or_else(bad)?;
        let handle = crate::command_client::CommandHandle::decode(bytes)?;
        if handle.profile() != "client"
            || hex::encode(handle.remote()) != self.identity.server_account
        {
            return Err(bad());
        }
        let file = self.workspace.join("lifecycle-handle.json");
        records::write_raw(&file, bytes)?;
        pair.unknown = false;
        Ok(Command {
            nonce,
            handle,
            handle_file: file,
            group_submit: None,
            began,
        })
    }
    async fn running(&self, pair: &Pair, command: &Command) -> Result<()> {
        use crate::command_wire as wire;
        let deadline = self
            .deadline
            .min(tokio::time::Instant::now() + std::time::Duration::from_secs(90));
        for _ in 0..30 {
            let output = self
                .call(
                    &self.selected(pair, "command-status", &command.handle_file)?,
                    Vec::new(),
                )
                .await?;
            if !output.status.success() || !output.stderr.is_empty() {
                return Err(bad());
            }
            let status: wire::Status = serde_json::from_slice(&output.stdout).map_err(|_| bad())?;
            let mut canonical = serde_json::to_vec(&status).map_err(|_| bad())?;
            canonical.push(b'\n');
            if canonical != output.stdout || status.invocation != command.handle.invocation() {
                return Err(bad());
            }
            if status.admission == wire::Admission::Engine
                && status.engine_known
                && status.state == wire::State::Running
                && status.input.accepted == 16384
                && status.input.delivered == 16384
                && status.input.eof_sent
                && status.input.health == wire::InputHealth::Closed
                && status.stdout.produced == 65536
                && status.stderr.produced == 65536
            {
                return Ok(());
            }
            if tokio::time::Instant::now() >= deadline {
                return Err(Error::Timeout);
            }
            tokio::time::sleep_until(
                (tokio::time::Instant::now() + std::time::Duration::from_secs(1)).min(deadline),
            )
            .await;
        }
        Err(Error::Timeout)
    }
    async fn bind(&self, pair: &mut Pair, command: &mut Command) -> Result<()> {
        let client = pair.client.as_mut().ok_or_else(bad)?;
        let digest = super::cleanup::client_group_submit(
            &mut client.child,
            &client.socket,
            &command.handle,
            self.deadline,
        )
        .await?;
        if command
            .group_submit
            .as_ref()
            .is_some_and(|old| old != &digest)
        {
            return Err(bad());
        }
        command.group_submit = Some(digest.clone());
        pair.expected.insert(
            (
                self.identity.client_account.clone(),
                hex::encode(command.handle.invocation()),
            ),
            (hex::encode(command.handle.session()), digest),
        );
        Ok(())
    }
    async fn finish(&self, pair: &mut Pair, command: &mut Command) -> Result<()> {
        let output = self
            .call(
                &self.selected(pair, "wait", &command.handle_file)?,
                Vec::new(),
            )
            .await;
        self.bind(pair, command).await?;
        let output = output?;
        let (_, stdout, stderr) = super::fixture::bytes(&command.nonce, 0)?;
        if output.status.code() != Some(7) || output.stdout != stdout || output.stderr != stderr {
            return Err(bad());
        }
        let receipt: serde_json::Value = records::read(
            &super::fixture::receipt_path(self.executable, &command.nonce)?,
            65536,
        )?;
        let index = ROLES
            .iter()
            .position(|r| *r == self.identity.role)
            .ok_or_else(bad)?;
        let expected = serde_json::to_value(super::fixture::expected_recovery_receipt(
            self.executable,
            &command.nonce,
            &self.ctx.candidate.tooling[index].files.fixture.sha256,
        )?)
        .map_err(|_| bad())?;
        if receipt != expected {
            return Err(bad());
        }
        Ok(())
    }
}

impl CommandClient<'_> {
    /// Retry the immutable invocation with a fresh protocol request ID. The CLI
    /// receipt does not expose the original request ID; this is not its replay.
    async fn retry_existing(&self, pair: &mut Pair, command: &Command) -> Result<()> {
        use crate::{command_local::LocalResult, command_wire as wire, ipc::Request};
        use base64::Engine as _;
        use std::os::unix::ffi::OsStrExt;
        let request_id = foundation::fresh_id(super::now()?)?;
        let mut argv = vec![self.executable.as_os_str().as_bytes().to_vec()];
        argv.extend(
            super::fixture::recovery_arguments(&command.nonce)?
                .into_iter()
                .map(|a| a.as_os_str().as_bytes().to_vec()),
        );
        let submit = wire::Submit {
            argv,
            invocation: command.handle.invocation(),
            stdin: super::fixture::bytes(&command.nonce, 0)?.0,
        };
        let expected = wire::Request {
            binding: wire::Binding {
                session: command.handle.session(),
                client: crate::command_config::account(&self.identity.client_account)?,
                server: crate::command_config::account(&self.identity.server_account)?,
            },
            id: hex::decode(&request_id)
                .map_err(|_| bad())?
                .try_into()
                .map_err(|_| bad())?,
            op: wire::RequestOp::Submit(submit.clone()),
        };
        let expected_digest = wire::request_digest(&wire::encode_request(&expected)?);
        let d = pair.client.as_mut().ok_or_else(bad)?;
        let first = super::cleanup::command_request(
            &mut d.child,
            &d.socket,
            &Request::CommandSubmit {
                remote: self.identity.server_account.clone(),
                relays: self.ctx.candidate.relays.clone(),
                session: hex::encode(command.handle.session()),
                request_id: request_id.clone(),
                submit,
            },
            self.deadline,
        )
        .await?;
        if !matches!(
            first,
            LocalResult::RemotePending { .. } | LocalResult::Answer { .. }
        ) {
            return Err(bad());
        }
        loop {
            let answer = super::cleanup::command_request(
                &mut d.child,
                &d.socket,
                &Request::CommandResponse {
                    handle: command.handle.clone(),
                    request_id: request_id.clone(),
                },
                self.deadline,
            )
            .await?;
            match answer {
                LocalResult::Answer {
                    handle,
                    request_id: rid,
                    response_base64,
                    response_sha256,
                } => {
                    let bytes = base64::engine::general_purpose::STANDARD
                        .decode(response_base64)
                        .map_err(|_| bad())?;
                    if handle != command.handle
                        || rid != request_id
                        || records::digest(&bytes) != response_sha256
                    {
                        return Err(bad());
                    }
                    let response = wire::decode_response(&bytes)?;
                    if response.binding != expected.binding
                        || response.id != expected.id
                        || response.op != wire::Op::Submit
                        || response.request_sha256 != expected_digest
                    {
                        return Err(bad());
                    }
                    if !matches!(
                        response.result,
                        Ok(wire::ResponseResult::Submit {
                            disposition: wire::Disposition::Existing,
                            ..
                        })
                    ) {
                        return Err(bad());
                    }
                    let consumed = super::cleanup::command_request(
                        &mut d.child,
                        &d.socket,
                        &Request::CommandConsume {
                            handle: command.handle.clone(),
                            request_id: rid,
                            response_sha256,
                            expected_cursors: None,
                        },
                        self.deadline,
                    )
                    .await?;
                    if !matches!(consumed, LocalResult::Consumed { .. }) {
                        return Err(bad());
                    }
                    return Ok(());
                }
                LocalResult::RemotePending { .. } => {}
                _ => return Err(bad()),
            }
            if tokio::time::Instant::now() >= self.deadline {
                return Err(Error::Timeout);
            }
            tokio::time::sleep_until(
                (tokio::time::Instant::now() + std::time::Duration::from_secs(1))
                    .min(self.deadline),
            )
            .await;
        }
    }
}
fn qualification_complete(operation_ok: bool, cleanup_ok: bool, cancelled: bool) -> bool {
    operation_ok && cleanup_ok && !cancelled
}
fn cleanup_allowed(unknown: bool, expected_len: usize) -> bool {
    !unknown && expected_len > 0
}
// Entry margins include the original 300-second cleanup reserve. Measure
// against the immutable runtime end, while individual operations still use work.
fn stage_budget(stage: Stage, provider_required: bool) -> Option<std::time::Duration> {
    let seconds = match (stage, provider_required) {
        (Stage::RollbackA, false) => 600,
        (Stage::RollbackA, true) => 1140,
        (Stage::RejectC, false) => 480,
        (Stage::RejectC, true) => 1020,
        (Stage::Provider, true) => 840,
        _ => return None,
    };
    Some(std::time::Duration::from_secs(seconds))
}
fn require_stage_budget(
    stage: Stage,
    provider_required: bool,
    end: tokio::time::Instant,
    now: tokio::time::Instant,
) -> Result<()> {
    if stage_budget(stage, provider_required)
        .is_some_and(|needed| end.saturating_duration_since(now) < needed)
    {
        return Err(Error::Timeout);
    }
    Ok(())
}
fn failed_inputs(ctx: &LifecycleContext, output: &Path, began: tokio::time::Instant) -> Result<()> {
    let mut result = LifecycleResult::new(ctx)?;
    result.failure = Some(Failure::Inputs);
    stamp(&mut result, Stage::Inputs, began, false);
    records::write(output, &result)
}
fn stamp(result: &mut LifecycleResult, stage: Stage, began: tokio::time::Instant, passed: bool) {
    let slot = result.stages.iter_mut().find(|s| s.stage == stage).unwrap();
    slot.disposition = if passed {
        Disposition::Passed
    } else {
        Disposition::Failed
    };
    slot.elapsed_ms = Some(began.elapsed().as_millis().min(1_800_000) as u64);
}

pub(super) async fn run_cli(
    context: &Path,
    workspace: &Path,
    output: &Path,
    candidate_assets: &Path,
    variant_assets: &Path,
    bootstrap: &Path,
    source_root: &Path,
) -> Result<()> {
    use std::time::Duration;
    let ctx: LifecycleContext = records::read(context, 65536)?;
    ctx.validate()?;
    let input_began = tokio::time::Instant::now();
    let mut result = LifecycleResult::new(&ctx)?;
    let prepared: Result<_> = (|| {
        let remaining = ctx
            .candidate
            .expires_at
            .checked_sub(super::now()?)
            .ok_or(Error::Timeout)?;
        if remaining < 1500 {
            return Err(Error::Timeout);
        }
        crate::identity::check_dir(workspace)?;
        if std::fs::read_dir(workspace)?.next().is_some() {
            return Err(bad());
        }
        let end = tokio::time::Instant::now() + Duration::from_secs(remaining);
        let variants = Variants::load(variant_assets, candidate_assets, &ctx)?;
        let term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
        let interrupt = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())?;
        Ok((end, variants, term, interrupt))
    })();
    let (end, variants, mut term, mut interrupt) = match prepared {
        Ok(value) => value,
        Err(error) => {
            failed_inputs(&ctx, output, input_began)?;
            return Err(error);
        }
    };
    let work = end - Duration::from_secs(300);
    let (signal_tx, signal_rx) = tokio::sync::watch::channel(false);
    let signal_task = tokio::spawn(async move {
        tokio::select! {_=term.recv()=>{},_=interrupt.recv()=>{}}
        let _ = signal_tx.send(true);
        std::future::pending::<()>().await;
    });
    let cancel = Cancellation(signal_rx);
    let inputs = Inputs {
        cancel: &cancel,
        ctx: &ctx,
        variants: &variants,
        candidate_assets,
        variant_assets,
        bootstrap,
    };
    let mut stage = Stage::Inputs;
    let mut began = input_began;
    let root = workspace.join("owned");
    let env = super::process::Environment {
        home: root.join("home"),
        temporary: root.join("tmp"),
        utilities: root.join("utilities"),
    };
    let mut pair: Option<Pair> = None;
    let mut negative: Option<Pair> = None;
    let negative_root = root.join("negative");
    let operation: Result<()> = async {
        cancel.check()?;
        directory(&root)?;
        for name in ["home", "tmp"] {
            directory(&root.join(name))?;
        }
        super::process::utilities(&env.utilities)?;
        if records::digest(&records::read_bytes(bootstrap, 128 * 1024)?)
            != ctx.candidate.bootstrap_sha256
        {
            return Err(bad());
        }
        cancel.check()?;
        stamp(&mut result, stage, began, true);
        stage = Stage::InstallA;
        began = tokio::time::Instant::now();
        let a = inputs.install(&root, &env, Some(&variants.a), work).await?;
        let identity = inputs.setup(&root, &env, &a, work).await?;
        let index = ROLES
            .iter()
            .position(|r| *r == identity.role)
            .ok_or_else(bad)?;
        let fixture = super::benchmark::copy_fixture(&root, &ctx.candidate, index)?;
        super::benchmark::configure(&root, &ctx.candidate, &identity, &fixture, &env, 3, work)
            .await?;
        let config = crate::command_config::CommandConfig::load(&root.join("profiles/server"))?;
        if config.limits().payload_retention.as_secs() < 1800 {
            return Err(bad());
        }
        let worker = RetainedFile::capture(&a.version_root.join("marmish-worker"))?;
        let helper = RetainedFile::capture(&a.version_root.join("marmish"))?;
        pair = Some(Pair::start(&root, &ctx, &env, end).await?);
        pair.as_mut().unwrap().ready(&root, work).await?;
        cancel.check()?;
        stamp(&mut result, stage, began, true);
        stage = Stage::RunningA;
        began = tokio::time::Instant::now();
        let client = CommandClient {
            cancel: &cancel,
            workspace: &root,
            ctx: &ctx,
            identity: &identity,
            environment: &env,
            executable: &fixture,
            deadline: work,
        };
        if end.saturating_duration_since(tokio::time::Instant::now()) < Duration::from_secs(1200) {
            return Err(Error::Timeout);
        }
        let mut command = client.submit(pair.as_mut().unwrap()).await?;
        client.running(pair.as_ref().unwrap(), &command).await?;
        client.bind(pair.as_mut().unwrap(), &mut command).await?;
        let marker = super::fixture::recovery_marker(&fixture, &command.nonce)?;
        let _held_marker = super::fixture::hold_recovery_marker(&fixture, &command.nonce, marker)?;
        pair.as_mut().unwrap().tree = Some(super::super::command_check::EngineTree::capture_path(
            &root.join("profiles/server"),
        )?);
        cancel.check()?;
        stamp(&mut result, stage, began, true);
        stage = Stage::UpgradeB;
        began = tokio::time::Instant::now();
        inputs.install(&root, &env, None, work).await?;
        worker.verify(&a.version_root.join("marmish-worker"))?;
        result.worker_inode_retained = true;
        helper.verify(&a.version_root.join("marmish"))?;
        result.helper_inode_retained = true;
        pair.as_mut().unwrap().stop_ordinary(&root, work).await?;
        cancel.check()?;
        stamp(&mut result, stage, began, true);
        stage = Stage::RecoverB;
        began = tokio::time::Instant::now();
        pair.as_mut()
            .unwrap()
            .restart(&root, &ctx, &env, work)
            .await?;
        result.restart_ready_ms = Some(began.elapsed().as_millis().max(1) as u64);
        super::fixture::verify_recovery_marker(&fixture, &command.nonce, marker)?;
        super::fixture::release_recovery(&fixture, &command.nonce)?;
        client.finish(pair.as_mut().unwrap(), &mut command).await?;
        result.command_wall_ms = Some(command.began.elapsed().as_millis().max(1) as u64);
        result.stdout_bytes = Some(131072);
        result.stderr_bytes = Some(131072);
        result.single_execution = true;
        result.finish_confirmed = true;
        client
            .retry_existing(pair.as_mut().unwrap(), &command)
            .await?;
        result.original_submit_existing = true;
        require_stage_budget(
            Stage::RollbackA,
            result.provider_required,
            end,
            tokio::time::Instant::now(),
        )?;
        pair.as_mut().unwrap().stop_ordinary(&root, work).await?;
        cancel.check()?;
        stamp(&mut result, stage, began, true);
        stage = Stage::RollbackA;
        began = tokio::time::Instant::now();
        pair.as_mut().unwrap().unknown = true;
        inputs.select(&root, &env, &a.manifest.id(), work).await?;
        worker.verify(&a.version_root.join("marmish-worker"))?;
        helper.verify(&a.version_root.join("marmish"))?;
        if root.join("bin/marmishd").canonicalize()?
            != a.version_root.join("marmishd").canonicalize()?
        {
            return Err(bad());
        }
        pair.as_mut()
            .unwrap()
            .restart(&root, &ctx, &env, work)
            .await?;
        worker.verify(&a.version_root.join("marmish-worker"))?;
        helper.verify(&root.join("bin/marmish").canonicalize()?)?;
        pair.as_ref()
            .unwrap()
            .tree
            .as_ref()
            .ok_or_else(bad)?
            .verify_path(&root.join("profiles/server"))?;
        // Reopening A must expose the same encrypted application history.
        let status = client
            .call(
                &client.selected(
                    pair.as_ref().unwrap(),
                    "command-status",
                    &command.handle_file,
                )?,
                Vec::new(),
            )
            .await?;
        let snapshot: crate::command_wire::Status =
            serde_json::from_slice(&status.stdout).map_err(|_| bad())?;
        if !status.status.success()
            || !status.stderr.is_empty()
            || snapshot.invocation != command.handle.invocation()
            || snapshot.state != crate::command_wire::State::Exited
        {
            return Err(bad());
        }
        cancel.check()?;
        pair.as_mut().unwrap().unknown = false;
        if pair.as_mut().unwrap().cleanup(&root, work, false).await != (true, true, true) {
            return Err(bad());
        }
        pair = None;
        result.engine_cleanup_complete = true;
        result.server_stop_complete = true;
        result.client_stop_complete = true;
        result.positive_rollback_loaded = true;
        cancel.check()?;
        stamp(&mut result, stage, began, true);
        stage = Stage::RejectC;
        began = tokio::time::Instant::now();
        require_stage_budget(stage, result.provider_required, end, began)?;
        // C is exercised against a separate genuine B-created account and
        // completed command; the positive rollback account stays untouched.
        directory(&negative_root)?;
        let b = inputs.install(&negative_root, &env, None, work).await?;
        let negative_identity = inputs.setup(&negative_root, &env, &b, work).await?;
        let negative_fixture =
            super::benchmark::copy_fixture(&negative_root, &ctx.candidate, index)?;
        super::benchmark::configure(
            &negative_root,
            &ctx.candidate,
            &negative_identity,
            &negative_fixture,
            &env,
            3,
            work,
        )
        .await?;
        negative = Some(Pair::start(&negative_root, &ctx, &env, end).await?);
        negative
            .as_mut()
            .unwrap()
            .ready(&negative_root, work)
            .await?;
        let negative_client = CommandClient {
            cancel: &cancel,
            workspace: &negative_root,
            ctx: &ctx,
            identity: &negative_identity,
            environment: &env,
            executable: &negative_fixture,
            deadline: work,
        };
        let mut negative_command = negative_client.submit(negative.as_mut().unwrap()).await?;
        negative_client
            .running(negative.as_ref().unwrap(), &negative_command)
            .await?;
        negative_client
            .bind(negative.as_mut().unwrap(), &mut negative_command)
            .await?;
        super::fixture::release_recovery(&negative_fixture, &negative_command.nonce)?;
        negative_client
            .finish(negative.as_mut().unwrap(), &mut negative_command)
            .await?;
        negative.as_mut().unwrap().tree =
            Some(super::super::command_check::EngineTree::capture_path(
                &negative_root.join("profiles/server"),
            )?);
        negative
            .as_mut()
            .unwrap()
            .stop_ordinary(&negative_root, work)
            .await?;
        let lease =
            crate::identity::ProfileLease::acquire(&negative_root.join("profiles"), "server")?;
        let inventory = DatabaseInventory::capture(lease.root())?;
        drop(lease);
        inputs
            .install(&negative_root, &env, Some(&variants.c), work)
            .await?;
        let socket = negative.as_ref().unwrap().sockets.join("negative");
        let mut args =
            super::matrix::base(&negative_root, "server", &socket, &ctx.candidate.relays);
        args.extend(["--max-uptime".into(), "15".into(), "daemon".into()]);
        let rejected = execute(
            &negative_root.join("bin/marmishd"),
            &args,
            &negative_root,
            &env,
            Vec::new(),
            work.min(tokio::time::Instant::now() + Duration::from_secs(20)),
            &cancel,
        )
        .await?;
        if rejected.status.success()
            || !rejected.stdout.is_empty()
            || rejected.stderr != b"marmish: storage: incompatible_state\n"
        {
            return Err(bad());
        }
        result.incompatible_start_refused = true;
        let lease =
            crate::identity::ProfileLease::acquire(&negative_root.join("profiles"), "server")?;
        inventory.verify()?;
        drop(lease);
        result.database_inventory_unchanged = true;
        inputs
            .select(&negative_root, &env, &b.manifest.id(), work)
            .await?;
        negative
            .as_mut()
            .unwrap()
            .restart(&negative_root, &ctx, &env, work)
            .await?;
        result.restored_b_loaded = true;
        cancel.check()?;
        // Seal and stop this live owner before provider work can consume its
        // original work deadline. The final cleanup must not reacquire it.
        if negative
            .as_mut()
            .unwrap()
            .cleanup(&negative_root, work, false)
            .await
            != (true, true, true)
        {
            return Err(bad());
        }
        negative = None;
        stamp(&mut result, stage, began, true);
        stage = Stage::Provider;
        began = tokio::time::Instant::now();
        if !result.provider_required {
            result.provider_cleanup_complete = true;
        } else {
            require_stage_budget(stage, true, end, began)?;
            let provider = super::lifecycle_provider::run(
                &ctx,
                source_root,
                bootstrap,
                super::lifecycle_provider::Assets {
                    candidate: candidate_assets,
                    variants: variant_assets,
                },
                &root.join("provider-result.json"),
                work.min(tokio::time::Instant::now() + Duration::from_secs(540)),
                &cancel,
            )
            .await?;
            result.provider_upgrade_loaded = provider.upgrade_loaded;
            result.provider_rollback_loaded = provider.rollback_loaded;
            result.provider_locked_denied = provider.locked_denied;
            result.provider_cleanup_complete = provider.cleanup_complete;
            if !provider.passed {
                result.failure =
                    Some(Failure::Provider(provider.failure.unwrap_or(
                        super::lifecycle_provider::ProviderFailure::InvalidReport,
                    )));
                return Err(bad());
            }
        }
        cancel.check()?;
        stamp(&mut result, stage, began, true);
        Ok(())
    }
    .await;
    if let Err(error) = &operation {
        if result.failure.is_none() {
            result.failure = Some(if cancel.check().is_err() {
                Failure::Interrupted
            } else if matches!(error, Error::Timeout) {
                Failure::Timeout
            } else {
                Failure::Runtime
            });
        }
        stamp(&mut result, stage, began, false);
    }
    let cleanup_began = tokio::time::Instant::now();
    if let Some(pair) = pair.as_mut() {
        let (e, s, c) = pair.cleanup(&root, end, operation.is_err()).await;
        result.engine_cleanup_complete = e;
        result.server_stop_complete = s;
        result.client_stop_complete = c;
    }
    if let Some(pair) = negative.as_mut() {
        let (e, s, c) = pair.cleanup(&negative_root, end, operation.is_err()).await;
        result.engine_cleanup_complete &= e;
        result.server_stop_complete &= s;
        result.client_stop_complete &= c;
    }
    if result.engine_cleanup_complete
        && result.server_stop_complete
        && result.client_stop_complete
        && result.provider_cleanup_complete
    {
        result.workspace_removed = std::fs::remove_dir_all(&root).is_ok();
    }
    let cleanup_ok = result.workspace_removed;
    stamp(&mut result, Stage::Cleanup, cleanup_began, cleanup_ok);
    tokio::task::yield_now().await;
    let interrupted = cancel.check().is_err();
    if interrupted {
        result.failure = Some(Failure::Interrupted);
    }
    if !cleanup_ok && result.failure.is_none() {
        result.failure = Some(Failure::Cleanup);
    }
    result.complete = qualification_complete(operation.is_ok(), cleanup_ok, interrupted);
    signal_task.abort();
    let _ = signal_task.await;
    if validate_record(&result, &ctx).is_err() {
        result = LifecycleResult::new(&ctx)?;
        result.failure = Some(Failure::Record);
        stamp(&mut result, Stage::Inputs, input_began, false);
    }
    records::write(output, &result)?;
    validate(&result, &ctx)
}

#[cfg(test)]
#[path = "installed_version_lifecycle_tests.rs"]
mod tests;
