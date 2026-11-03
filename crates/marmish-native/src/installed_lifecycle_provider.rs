//! Adapter to the separately reviewed disposable-account provider supervisor.
use super::{
    process, records,
    version_lifecycle::{Cancellation, LifecycleContext},
};
use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use std::{ffi::OsString, path::Path, time::Duration};
use tokio::time::Instant;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum ProviderFailure {
    UpgradeLockedOrDenied,
    UpgradeReloadFailed,
    OperationTimeout,
    Interrupted,
    QualificationFailed,
    PrerequisiteFailed,
    CleanupIncomplete,
    InvalidReport,
    ChildFailed,
    DrainOverflow,
    Deadline,
}

pub(super) struct ProviderOutcome {
    pub(super) passed: bool,
    pub(super) failure: Option<ProviderFailure>,
    pub(super) upgrade_loaded: bool,
    pub(super) rollback_loaded: bool,
    pub(super) locked_denied: bool,
    pub(super) cleanup_complete: bool,
}
impl ProviderOutcome {
    fn failed(failure: ProviderFailure) -> Self {
        Self {
            passed: false,
            failure: Some(failure),
            upgrade_loaded: false,
            rollback_loaded: false,
            locked_denied: false,
            cleanup_complete: false,
        }
    }
    fn fail(&mut self, failure: ProviderFailure) {
        self.passed = false;
        self.failure = Some(failure);
    }
    fn observe(&mut self, failure: Option<ProviderFailure>) {
        if let Some(failure) = failure {
            // A finite supervisor refusal normally exits nonzero. Preserve its
            // validated category and independent cleanup facts.
            if failure != ProviderFailure::ChildFailed
                || self.passed
                || self.failure == Some(ProviderFailure::InvalidReport)
            {
                self.fail(failure);
            }
        }
    }
}

fn bad() -> Error {
    Error::Invalid("lifecycle_provider")
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Report {
    schema: u8,
    kind: String,
    ok: bool,
    source_commit: String,
    target: String,
    #[serde(default)]
    lifecycle_stage: Option<String>,
    #[serde(default)]
    lifecycle_upgrade_loaded: bool,
    #[serde(default)]
    lifecycle_rollback_loaded: bool,
    #[serde(default)]
    lifecycle_locked_denied: bool,
    #[serde(default)]
    owned_account_removed: bool,
    #[serde(default)]
    runner_keychain_unchanged: bool,
    #[serde(default)]
    supervisor_interpreter_unchanged: bool,
    #[serde(default)]
    python_in_runtime_path: Option<bool>,
    #[serde(default)]
    network_activation: Option<bool>,
    #[serde(default)]
    owned_children_reaped: bool,
    #[serde(default)]
    owned_state_removed: bool,
    elapsed_ms: u64,
    #[serde(default)]
    failure: Option<String>,
    #[serde(default)]
    run_id: Option<String>,
    #[serde(default)]
    run_attempt: Option<String>,
    provider_packages: Vec<String>,
}

// Exact finite source labels; unknown strings cannot become trusted diagnostics.
const REPORT_FAILURES: &[(&str, ProviderFailure)] = &[
    ("absent_auto_files", ProviderFailure::QualificationFailed),
    (
        "absent_existing_accepted",
        ProviderFailure::QualificationFailed,
    ),
    (
        "absent_existing_files_fallback",
        ProviderFailure::QualificationFailed,
    ),
    (
        "absent_profile_mutated",
        ProviderFailure::QualificationFailed,
    ),
    (
        "absent_provider_category",
        ProviderFailure::QualificationFailed,
    ),
    (
        "child_cleanup_incomplete",
        ProviderFailure::CleanupIncomplete,
    ),
    (
        "forbidden_helper_operation",
        ProviderFailure::QualificationFailed,
    ),
    (
        "fresh_identity_reload",
        ProviderFailure::QualificationFailed,
    ),
    ("helper_contract", ProviderFailure::QualificationFailed),
    ("input_size", ProviderFailure::PrerequisiteFailed),
    ("interpreter_changed", ProviderFailure::PrerequisiteFailed),
    (
        "keychain_cleanup_incomplete",
        ProviderFailure::CleanupIncomplete,
    ),
    (
        "lifecycle_a_auto_keychain",
        ProviderFailure::QualificationFailed,
    ),
    ("lifecycle_a_binary", ProviderFailure::QualificationFailed),
    (
        "lifecycle_a_manifest_pin",
        ProviderFailure::QualificationFailed,
    ),
    ("lifecycle_arguments", ProviderFailure::PrerequisiteFailed),
    ("lifecycle_asset_set", ProviderFailure::PrerequisiteFailed),
    ("lifecycle_b_version", ProviderFailure::QualificationFailed),
    (
        "lifecycle_locked_existing_accepted",
        ProviderFailure::QualificationFailed,
    ),
    (
        "lifecycle_locked_fallback",
        ProviderFailure::QualificationFailed,
    ),
    (
        "lifecycle_locked_fresh_accepted",
        ProviderFailure::QualificationFailed,
    ),
    (
        "lifecycle_locked_items_changed",
        ProviderFailure::QualificationFailed,
    ),
    (
        "lifecycle_old_helper_retained",
        ProviderFailure::QualificationFailed,
    ),
    (
        "lifecycle_profile_changed",
        ProviderFailure::QualificationFailed,
    ),
    (
        "lifecycle_provider_deadline",
        ProviderFailure::OperationTimeout,
    ),
    (
        "lifecycle_provider_incomplete",
        ProviderFailure::QualificationFailed,
    ),
    ("lifecycle_provider_signal", ProviderFailure::Interrupted),
    (
        "lifecycle_rollback_items_changed",
        ProviderFailure::QualificationFailed,
    ),
    (
        "lifecycle_selected_version",
        ProviderFailure::QualificationFailed,
    ),
    (
        "lifecycle_upgrade_identity",
        ProviderFailure::QualificationFailed,
    ),
    (
        "lifecycle_upgrade_items_changed",
        ProviderFailure::QualificationFailed,
    ),
    (
        "lifecycle_upgrade_locked_or_denied",
        ProviderFailure::UpgradeLockedOrDenied,
    ),
    (
        "lifecycle_upgrade_reload_failed",
        ProviderFailure::UpgradeReloadFailed,
    ),
    (
        "locked_existing_accepted",
        ProviderFailure::QualificationFailed,
    ),
    (
        "locked_files_fallback",
        ProviderFailure::QualificationFailed,
    ),
    (
        "locked_fresh_accepted",
        ProviderFailure::QualificationFailed,
    ),
    (
        "locked_native_items_changed",
        ProviderFailure::QualificationFailed,
    ),
    (
        "locked_profile_mutated",
        ProviderFailure::QualificationFailed,
    ),
    (
        "named_identity_separation",
        ProviderFailure::QualificationFailed,
    ),
    (
        "named_reference_separation",
        ProviderFailure::QualificationFailed,
    ),
    ("operation_failed", ProviderFailure::QualificationFailed),
    ("operation_input_limit", ProviderFailure::PrerequisiteFailed),
    (
        "operation_output_limit",
        ProviderFailure::QualificationFailed,
    ),
    ("operation_timeout", ProviderFailure::OperationTimeout),
    (
        "owned_account_already_exists",
        ProviderFailure::PrerequisiteFailed,
    ),
    (
        "owned_account_children_remain",
        ProviderFailure::CleanupIncomplete,
    ),
    (
        "owned_account_cleanup_deadline",
        ProviderFailure::OperationTimeout,
    ),
    (
        "owned_account_cleanup_identity",
        ProviderFailure::CleanupIncomplete,
    ),
    (
        "owned_account_cleanup_incomplete",
        ProviderFailure::CleanupIncomplete,
    ),
    (
        "owned_account_context_required",
        ProviderFailure::PrerequisiteFailed,
    ),
    (
        "owned_account_home_remains",
        ProviderFailure::CleanupIncomplete,
    ),
    (
        "owned_account_identity",
        ProviderFailure::PrerequisiteFailed,
    ),
    (
        "owned_account_interpreter",
        ProviderFailure::PrerequisiteFailed,
    ),
    (
        "owned_account_prerequisite_failed",
        ProviderFailure::PrerequisiteFailed,
    ),
    (
        "owned_account_qualification_failed",
        ProviderFailure::QualificationFailed,
    ),
    ("owned_account_report", ProviderFailure::PrerequisiteFailed),
    (
        "owned_account_report_limit",
        ProviderFailure::PrerequisiteFailed,
    ),
    (
        "owned_account_setup_deadline",
        ProviderFailure::OperationTimeout,
    ),
    (
        "owned_account_uid_exhausted",
        ProviderFailure::PrerequisiteFailed,
    ),
    (
        "owned_default_keychain",
        ProviderFailure::QualificationFailed,
    ),
    (
        "owned_descendant_remained",
        ProviderFailure::CleanupIncomplete,
    ),
    (
        "owned_home_already_exists",
        ProviderFailure::PrerequisiteFailed,
    ),
    (
        "owned_provider_service_state",
        ProviderFailure::QualificationFailed,
    ),
    (
        "package_metadata_invalid",
        ProviderFailure::PrerequisiteFailed,
    ),
    (
        "package_metadata_limit",
        ProviderFailure::PrerequisiteFailed,
    ),
    ("plaintext_fallback", ProviderFailure::QualificationFailed),
    ("private_bus_address", ProviderFailure::QualificationFailed),
    ("private_bus_socket", ProviderFailure::QualificationFailed),
    ("private_bus_start", ProviderFailure::QualificationFailed),
    ("private_mode", ProviderFailure::PrerequisiteFailed),
    ("profile_custody", ProviderFailure::QualificationFailed),
    ("profile_metadata", ProviderFailure::QualificationFailed),
    (
        "provider_cleanup_incomplete",
        ProviderFailure::CleanupIncomplete,
    ),
    ("provider_item_count", ProviderFailure::QualificationFailed),
    (
        "provider_item_uniqueness",
        ProviderFailure::QualificationFailed,
    ),
    ("provider_list", ProviderFailure::QualificationFailed),
    (
        "provider_metadata_limit",
        ProviderFailure::QualificationFailed,
    ),
    (
        "provider_prerequisite",
        ProviderFailure::QualificationFailed,
    ),
    (
        "provider_qualification_failed",
        ProviderFailure::QualificationFailed,
    ),
    (
        "provider_state_unavailable",
        ProviderFailure::QualificationFailed,
    ),
    (
        "qualification_budget_invalid",
        ProviderFailure::PrerequisiteFailed,
    ),
    ("qualification_deadline", ProviderFailure::OperationTimeout),
    (
        "qualification_prerequisite_failed",
        ProviderFailure::PrerequisiteFailed,
    ),
    (
        "qualification_source_missing",
        ProviderFailure::PrerequisiteFailed,
    ),
    (
        "qualification_target_mismatch",
        ProviderFailure::PrerequisiteFailed,
    ),
    (
        "quickstart_auto_keychain",
        ProviderFailure::QualificationFailed,
    ),
    ("quickstart_report", ProviderFailure::QualificationFailed),
    (
        "separate_key_references",
        ProviderFailure::QualificationFailed,
    ),
    ("staged_input_changed", ProviderFailure::PrerequisiteFailed),
    ("staged_input_count", ProviderFailure::PrerequisiteFailed),
    ("staged_pins_required", ProviderFailure::PrerequisiteFailed),
    ("unsafe_input", ProviderFailure::PrerequisiteFailed),
];

fn report_failure(label: &str) -> Result<ProviderFailure> {
    REPORT_FAILURES
        .iter()
        .find_map(|(known, failure)| (*known == label).then_some(*failure))
        .ok_or_else(bad)
}

fn outcome(bytes: &[u8], ctx: &LifecycleContext, budget: u64) -> Result<ProviderOutcome> {
    let report: Report = serde_json::from_slice(bytes).map_err(|_| bad())?;
    if report.schema != 1
        || report.kind != "installed_auto_provider"
        || report.source_commit != ctx.candidate.source_commit
        || report.target != ctx.target
        || report.python_in_runtime_path == Some(true)
        || report.network_activation == Some(true)
        || report.elapsed_ms > budget * 1000
        || report.provider_packages.len() > 8
        || report.provider_packages.iter().any(|s| s.len() > 256)
        || report.run_id.is_some()
        || report.run_attempt.is_some()
        || report.lifecycle_stage.as_deref().is_some_and(|stage| {
            !matches!(
                stage,
                "initial_setup"
                    | "upgrade_install"
                    | "upgrade_reload"
                    | "rollback_reload"
                    | "locked_denial"
                    | "complete"
            )
        })
    {
        return Err(bad());
    }
    let cleanup_complete = report.owned_account_removed
        && report.runner_keychain_unchanged
        && report.supervisor_interpreter_unchanged
        && report.owned_children_reaped
        && report.owned_state_removed;
    let failure = if report.ok {
        if report.failure.is_some()
            || report.python_in_runtime_path != Some(false)
            || report.network_activation != Some(false)
            || report.lifecycle_stage.as_deref() != Some("complete")
            || !report.lifecycle_upgrade_loaded
            || !report.lifecycle_rollback_loaded
            || !report.lifecycle_locked_denied
            || !cleanup_complete
        {
            return Err(bad());
        }
        None
    } else {
        Some(report_failure(report.failure.as_deref().ok_or_else(bad)?)?)
    };
    Ok(ProviderOutcome {
        passed: report.ok,
        failure,
        upgrade_loaded: report.lifecycle_upgrade_loaded,
        rollback_loaded: report.lifecycle_rollback_loaded,
        locked_denied: report.lifecycle_locked_denied,
        cleanup_complete,
    })
}

fn budget(deadline: Instant) -> Result<u64> {
    let seconds = deadline
        .saturating_duration_since(Instant::now())
        .as_secs()
        .min(540);
    if seconds <= 120 {
        return Err(Error::Timeout);
    }
    Ok(seconds)
}

pub(super) struct Assets<'a> {
    pub(super) candidate: &'a Path,
    pub(super) variants: &'a Path,
}

fn version_argument(ctx: &LifecycleContext) -> Result<(&'static str, OsString)> {
    ctx.validate()?;
    Ok((
        "--lifecycle-b-version",
        ctx.candidate.version.clone().into(),
    ))
}

pub(super) async fn run(
    ctx: &LifecycleContext,
    source_root: &Path,
    bootstrap: &Path,
    assets: Assets<'_>,
    report: &Path,
    deadline: Instant,
    cancel: &Cancellation,
) -> Result<ProviderOutcome> {
    ctx.validate()?;
    if cancel.check().is_err() {
        return Ok(ProviderOutcome::failed(ProviderFailure::Interrupted));
    }
    if ctx.target != "aarch64-apple-darwin" || report.exists() {
        return Err(bad());
    }
    let variants =
        super::version_lifecycle::Variants::load(assets.variants, assets.candidate, ctx)?;
    // Leave ten seconds inside the original watchdog for TERM/reap/drains.
    let work = deadline
        .checked_sub(Duration::from_secs(10))
        .ok_or(Error::Timeout)?;
    let seconds = budget(work)?;
    let script = source_root.join("tools/marmish_native_keychain_check.py");
    let mut args = vec![OsString::from("-I"), script.into_os_string()];
    for (name, value) in [
        ("--bootstrap", bootstrap.as_os_str().to_owned()),
        ("--assets-dir", assets.candidate.as_os_str().to_owned()),
        ("--report", report.as_os_str().to_owned()),
        (
            "--source-commit",
            ctx.candidate.source_commit.clone().into(),
        ),
        ("--target", ctx.target.clone().into()),
        version_argument(ctx)?,
        (
            "--lifecycle-a-assets",
            assets.variants.join("a").into_os_string(),
        ),
        (
            "--lifecycle-a-manifest-sha256",
            variants.a.manifest_sha256.into(),
        ),
        ("--deadline-seconds", seconds.to_string().into()),
    ] {
        args.push(name.into());
        args.push(value);
    }
    let parent = report.parent().ok_or_else(bad)?;
    crate::identity::check_dir(parent)?;
    let environment = process::Environment {
        home: std::env::var_os("HOME").ok_or_else(bad)?.into(),
        temporary: parent.into(),
        utilities: "/usr/bin:/bin:/usr/sbin:/sbin".into(),
    };
    let command = process::command(
        Path::new("/usr/bin/python3"),
        &args,
        source_root,
        &environment,
    );
    let observation = supervise(command, work, deadline, cancel).await;
    let mut result = records::read_bytes(report, 16384)
        .and_then(|bytes| outcome(&bytes, ctx, seconds))
        .unwrap_or_else(|_| ProviderOutcome::failed(ProviderFailure::InvalidReport));
    result.observe(observation);
    if cancel.check().is_err() {
        result.fail(ProviderFailure::Interrupted);
    }
    Ok(result)
}

/// Owns the leader, its process group and both drains until all are joined.
/// Tests inject only this command; production always selects the pinned supervisor above.
async fn supervise(
    mut command: tokio::process::Command,
    work: Instant,
    deadline: Instant,
    cancel: &Cancellation,
) -> Option<ProviderFailure> {
    use ProviderFailure::*;
    if cancel.check().is_err() {
        return Some(Interrupted);
    }
    if Instant::now() >= work {
        return Some(Deadline);
    }
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(_) => return Some(ChildFailed),
    };
    let mut stdout = tokio::spawn(process::drain(
        child.stdout.take().expect("piped supervisor stdout"),
        65536,
    ));
    let mut stderr = tokio::spawn(process::drain(
        child.stderr.take().expect("piped supervisor stderr"),
        65536,
    ));
    let mut failure = tokio::select! {
        result = child.wait() => match result {
            Ok(status) if status.success() => None,
            _ => Some(ChildFailed),
        },
        _ = cancel.cancelled() => Some(Interrupted),
        _ = tokio::time::sleep_until(work) => Some(Deadline),
    };
    if failure.is_some() {
        if let Some(pid) = child.id() {
            // The unreaped owned leader fences this process-group identity.
            unsafe {
                libc::kill(-(pid as i32), libc::SIGTERM);
            }
        }
        let grace = deadline.min(Instant::now() + Duration::from_secs(60));
        if !matches!(
            tokio::time::timeout_at(grace, child.wait()).await,
            Ok(Ok(_))
        ) && process::terminate(&mut child).await.is_err()
        {
            failure = Some(ChildFailed);
        }
    }
    let drains = tokio::time::timeout_at(deadline, async {
        let out = (&mut stdout).await.map_err(|_| bad())??;
        let err = (&mut stderr).await.map_err(|_| bad())??;
        Ok::<_, Error>(out.1 || err.1)
    })
    .await;
    match drains {
        Ok(Ok(false)) => {}
        Ok(Ok(true)) => failure = Some(DrainOverflow),
        _ => {
            stdout.abort();
            stderr.abort();
            let _ = stdout.await;
            let _ = stderr.await;
            failure = Some(ChildFailed);
        }
    }
    failure
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context() -> LifecycleContext {
        let mut candidate = records::tests::ctx();
        candidate.cohort = "version-lifecycle".into();
        candidate.version = "0.2.0".into();
        candidate.expires_at = candidate.created_at + 1800;
        LifecycleContext::mint(
            candidate,
            "aarch64-apple-darwin".into(),
            "4".repeat(64),
            3000,
            150,
        )
        .unwrap()
    }
    #[test]
    fn provider_forwards_exact_validated_candidate_version() {
        for version in ["0.2.0", "0.2.1", "0.2.12"] {
            let mut ctx = context();
            ctx.candidate.version = version.into();
            let (flag, value) = version_argument(&ctx).unwrap();
            assert_eq!(flag, "--lifecycle-b-version");
            assert_eq!(value, OsString::from(version));
        }
        let mut ctx = context();
        ctx.candidate.version = "0.2.1-rc.1".into();
        assert!(version_argument(&ctx).is_err());
    }

    fn valid_report(ctx: &LifecycleContext) -> serde_json::Value {
        serde_json::json!({"schema":1,"kind":"installed_auto_provider","ok":true,
            "source_commit":ctx.candidate.source_commit,"target":ctx.target,"lifecycle_stage":"complete",
            "lifecycle_upgrade_loaded":true,"lifecycle_rollback_loaded":true,"lifecycle_locked_denied":true,
            "owned_account_removed":true,"runner_keychain_unchanged":true,"supervisor_interpreter_unchanged":true,
            "python_in_runtime_path":false,"network_activation":false,"owned_children_reaped":true,
            "owned_state_removed":true,"elapsed_ms":500,"run_id":null,"run_attempt":null,"provider_packages":[]})
    }
    #[test]
    fn provider_requires_all_exact_provenance_execution_and_cleanup_facts() {
        let ctx = context();
        let value = valid_report(&ctx);
        let parse = |v: &serde_json::Value| outcome(&serde_json::to_vec(v).unwrap(), &ctx, 540);
        assert!(parse(&value).is_ok());
        for key in ["python_in_runtime_path", "network_activation"] {
            let mut changed = value.clone();
            changed.as_object_mut().unwrap().remove(key);
            assert!(parse(&changed).is_err(), "missing {key}");
        }
        for key in [
            "ok",
            "lifecycle_upgrade_loaded",
            "lifecycle_rollback_loaded",
            "lifecycle_locked_denied",
            "owned_account_removed",
            "runner_keychain_unchanged",
            "supervisor_interpreter_unchanged",
            "owned_children_reaped",
            "owned_state_removed",
        ] {
            let mut changed = value.clone();
            changed[key] = false.into();
            assert!(parse(&changed).is_err(), "{key}");
        }
        for (key, v) in [
            ("source_commit", serde_json::json!("9".repeat(40))),
            ("target", serde_json::json!("x86_64-unknown-linux-gnu")),
            ("lifecycle_stage", serde_json::json!("upgrade_reload")),
            ("elapsed_ms", serde_json::json!(540001)),
            ("unexpected", serde_json::json!(true)),
        ] {
            let mut changed = value.clone();
            changed[key] = v;
            assert!(parse(&changed).is_err(), "{key}");
        }
    }
    #[tokio::test(start_paused = true)]
    async fn provider_budget_is_clipped_without_extending_original_deadline() {
        assert_eq!(
            budget(Instant::now() + Duration::from_secs(900)).unwrap(),
            540
        );
        assert_eq!(
            budget(Instant::now() + Duration::from_secs(121)).unwrap(),
            121
        );
        assert!(budget(Instant::now() + Duration::from_secs(120)).is_err());
    }

    #[test]
    fn failed_report_preserves_finite_category_and_independent_cleanup_facts() {
        let ctx = context();
        let mut value = valid_report(&ctx);
        value["ok"] = false.into();
        value["failure"] = "lifecycle_upgrade_locked_or_denied".into();
        value["lifecycle_stage"] = "upgrade_reload".into();
        value["lifecycle_upgrade_loaded"] = false.into();
        value["lifecycle_rollback_loaded"] = false.into();
        value["lifecycle_locked_denied"] = false.into();
        let mut result = outcome(&serde_json::to_vec(&value).unwrap(), &ctx, 540).unwrap();
        result.observe(Some(ProviderFailure::ChildFailed));
        assert!(!result.passed);
        assert_eq!(result.failure, Some(ProviderFailure::UpgradeLockedOrDenied));
        assert!(result.cleanup_complete);
        value["owned_account_removed"] = false.into();
        assert!(
            !outcome(&serde_json::to_vec(&value).unwrap(), &ctx, 540)
                .unwrap()
                .cleanup_complete
        );
        value["failure"] = "arbitrary untrusted failure".into();
        assert!(outcome(&serde_json::to_vec(&value).unwrap(), &ctx, 540).is_err());
        value["failure"] = "operation_timeout".into();
        value["source_commit"] = "9".repeat(40).into();
        assert!(outcome(&serde_json::to_vec(&value).unwrap(), &ctx, 540).is_err());
    }

    #[test]
    fn supervisor_failure_labels_match_python_ast_and_retain_cleanup() {
        // Parse syntax only: do not import or execute the provider supervisor.
        let source = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tools/marmish_native_keychain_check.py");
        let output = std::process::Command::new("/usr/bin/python3")
            .args(["-I", "-c", r#"import ast,json,sys
root=ast.parse(open(sys.argv[1],encoding="utf-8").read())
labels=set()
def add(node):
    if isinstance(node,ast.Constant) and isinstance(node.value,str): labels.add(node.value)
for node in ast.walk(root):
    if isinstance(node,ast.Call):
        name=node.func.id if isinstance(node.func,ast.Name) else node.func.attr if isinstance(node.func,ast.Attribute) else ""
        if name=="require" and len(node.args)>1: add(node.args[1])
        if name=="Refused" and node.args: add(node.args[0])
        if name=="setdefault" and len(node.args)>1 and isinstance(node.args[0],ast.Constant) and node.args[0].value=="failure": add(node.args[1])
        for item in node.keywords:
            if item.arg=="failure": add(item.value)
            if item.arg=="error_map" and isinstance(item.value,ast.Dict):
                for value in item.value.values: add(value)
    if isinstance(node,ast.Assign):
        if any(isinstance(target,ast.Subscript) and isinstance(target.slice,ast.Constant) and target.slice.value=="failure" for target in node.targets): add(node.value)
        if any(isinstance(target,ast.Name) and target.id=="category" for target in node.targets) and isinstance(node.value,ast.IfExp): add(node.value.orelse)
    if isinstance(node,ast.Dict):
        for key,value in zip(node.keys,node.values):
            if isinstance(key,ast.Constant) and key.value=="failure": add(value)
print(json.dumps(sorted(labels)))
"#])
            .arg(source).output().unwrap();
        assert!(output.status.success());
        assert!(output.stdout.len() <= 16 * 1024);
        let python: std::collections::BTreeSet<String> =
            serde_json::from_slice(&output.stdout).unwrap();
        let rust: std::collections::BTreeSet<String> = REPORT_FAILURES
            .iter()
            .map(|(label, _)| (*label).into())
            .collect();
        assert_eq!(python, rust);
        assert_eq!(rust.len(), REPORT_FAILURES.len());
        let ctx = context();
        for (label, failure) in REPORT_FAILURES {
            let mut report = valid_report(&ctx);
            report["ok"] = false.into();
            report["failure"] = (*label).into();
            let mut parsed = outcome(&serde_json::to_vec(&report).unwrap(), &ctx, 540).unwrap();
            parsed.observe(Some(ProviderFailure::ChildFailed));
            assert!(!parsed.passed, "{label}");
            assert_eq!(parsed.failure, Some(*failure), "{label}");
            assert!(parsed.cleanup_complete, "{label}");
        }
        assert!(report_failure("unrecognized_provider_error").is_err());
    }

    fn shell(directory: &Path, script: &str) -> tokio::process::Command {
        process::command(
            Path::new("/bin/sh"),
            &[
                "-c".into(),
                script.into(),
                "provider-test".into(),
                directory.join("pids").into_os_string(),
            ],
            directory,
            &process::Environment {
                home: directory.into(),
                temporary: directory.into(),
                utilities: "/usr/bin:/bin".into(),
            },
        )
    }
    async fn ready(directory: &Path) -> Vec<i32> {
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                if let Ok(text) = std::fs::read_to_string(directory.join("pids")) {
                    let pids: Vec<i32> = text
                        .split_whitespace()
                        .map(|v| v.parse().unwrap())
                        .collect();
                    if !pids.is_empty() {
                        break pids;
                    }
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap()
    }
    fn group_gone(pid: i32) {
        assert_eq!(unsafe { libc::kill(-pid, 0) }, -1);
        assert_eq!(
            std::io::Error::last_os_error().raw_os_error(),
            Some(libc::ESRCH)
        );
    }
    #[tokio::test]
    async fn prior_cancellation_prevents_spawn_and_cannot_become_success() {
        let d = tempfile::tempdir().unwrap();
        let (tx, cancel) = Cancellation::channel();
        tx.send(true).unwrap();
        let end = Instant::now() + Duration::from_secs(2);
        assert_eq!(
            supervise(shell(d.path(), "echo $$ > \"$1\""), end, end, &cancel).await,
            Some(ProviderFailure::Interrupted)
        );
        assert!(!d.path().join("pids").exists());
        let mut report = outcome(
            &serde_json::to_vec(&valid_report(&context())).unwrap(),
            &context(),
            540,
        )
        .unwrap();
        report.observe(Some(ProviderFailure::Interrupted));
        assert!(!report.passed);
    }
    #[tokio::test]
    async fn cancellation_terms_group_and_joins_leader_and_both_drains() {
        let d = tempfile::tempdir().unwrap();
        let (tx, cancel) = Cancellation::channel();
        let end = Instant::now() + Duration::from_secs(6);
        let command = shell(
            d.path(),
            "/bin/sleep 30 & child=$!; trap 'kill \"$child\"; wait \"$child\"; exit 0' TERM; echo \"$$ $child\" > \"$1\"; wait \"$child\"",
        );
        let task = tokio::spawn(async move { supervise(command, end, end, &cancel).await });
        let pids = ready(d.path()).await;
        tx.send(true).unwrap();
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(3), task)
                .await
                .unwrap()
                .unwrap(),
            Some(ProviderFailure::Interrupted)
        );
        group_gone(pids[0]);
        assert_eq!(unsafe { libc::kill(pids[1], 0) }, -1);
    }
    #[tokio::test]
    async fn original_deadline_kills_term_ignoring_owned_leader_and_reaps_it() {
        let d = tempfile::tempdir().unwrap();
        let (_tx, cancel) = Cancellation::channel();
        let work = Instant::now() + Duration::from_secs(1);
        let end = work + Duration::from_millis(200);
        let command = shell(
            d.path(),
            "trap '' TERM; echo $$ > \"$1\"; while :; do :; done",
        );
        let task = tokio::spawn(async move { supervise(command, work, end, &cancel).await });
        let pids = ready(d.path()).await;
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(3), task)
                .await
                .unwrap()
                .unwrap(),
            Some(ProviderFailure::Deadline)
        );
        group_gone(pids[0]);
    }
    #[tokio::test]
    async fn bounded_drains_reject_overflow_and_nonzero_without_a_report() {
        let d = tempfile::tempdir().unwrap();
        let (_tx, cancel) = Cancellation::channel();
        let end = Instant::now() + Duration::from_secs(5);
        assert_eq!(supervise(shell(d.path(),
            "i=0; while [ \"$i\" -lt 7000 ]; do printf 0123456789; printf 0123456789 >&2; i=$((i+1)); done"), end, end, &cancel).await,
            Some(ProviderFailure::DrainOverflow));
        assert_eq!(
            supervise(shell(d.path(), "exit 7"), end, end, &cancel).await,
            Some(ProviderFailure::ChildFailed)
        );
    }
}
