use super::*;
use std::os::unix::fs::PermissionsExt;

fn private_test_root() -> tempfile::TempDir {
    let parent = std::env::temp_dir().canonicalize().unwrap();
    tempfile::Builder::new().tempdir_in(parent).unwrap()
}

fn context(index: usize) -> LifecycleContext {
    let mut candidate = records::tests::ctx();
    candidate.cohort = "version-lifecycle".into();
    candidate.version = "0.2.0".into();
    candidate.expires_at = candidate.created_at + 1800;
    LifecycleContext::mint(candidate, TARGETS[index].into(), "4".repeat(64), 3000, 150).unwrap()
}

fn variants(ctx: &LifecycleContext) -> Variants {
    let variant = |version: &str, digit: char| Variant {
        version: version.into(),
        commit: digit.to_string().repeat(40),
        tree: digit.to_string().repeat(40),
        patch_sha256: "7".repeat(64),
        manifest_sha256: "8".repeat(64),
        signing_kind: if ctx.target == TARGETS[2] {
            "adhoc"
        } else {
            "none"
        }
        .into(),
        build_ms: 1,
    };
    Variants {
        schema: 1,
        source_commit: ctx.candidate.source_commit.clone(),
        target: ctx.target.clone(),
        b_manifest_sha256: ctx.candidate.manifests[ctx.role().unwrap()].clone(),
        a: variant(&format!("{}-rc.1", ctx.candidate.version), '5'),
        c: variant(&format!("{}-rc.2", ctx.candidate.version), '6'),
    }
}

#[test]
fn stable_patch_versions_require_exact_derived_variants() {
    for index in 0..3 {
        for version in [
            "0.2.0",
            "0.2.1",
            "0.2.12",
            "0.2.18446744073709551615",
            "0.3.0",
            "0.3.1",
            "0.3.12",
            "0.3.18446744073709551615",
        ] {
            let mut ctx = context(index);
            ctx.candidate.version = version.into();
            ctx.validate().unwrap();
            let valid = variants(&ctx);
            valid.validate(&ctx).unwrap();
            let mut wrong = valid.clone();
            std::mem::swap(&mut wrong.a.version, &mut wrong.c.version);
            assert!(wrong.validate(&ctx).is_err());
            wrong = valid;
            wrong.a.version = version.into();
            assert!(wrong.validate(&ctx).is_err());
        }
    }
    for version in [
        "0.2.01",
        "0.2.18446744073709551616",
        "0.2.100000000000000000000",
        "0.2.1-rc.1",
        "0.2.1+build",
        "0.4.1",
        "0.2.1\n",
    ] {
        let mut ctx = context(0);
        ctx.candidate.version = version.into();
        assert!(ctx.validate().is_err());
    }
}

#[test]
fn mint_preserves_original_cutoff_and_requires_cleanup_margin() {
    let original = context(0);
    let later = LifecycleContext::mint(
        original.candidate.clone(),
        original.target.clone(),
        original.variants_sha256.clone(),
        original.internal_deadline,
        900,
    )
    .unwrap();
    assert_eq!(later.candidate.created_at, original.candidate.created_at);
    assert_eq!(later.candidate.expires_at, original.candidate.expires_at);
    assert_eq!(later.digest().unwrap(), original.digest().unwrap());
    assert!(
        LifecycleContext::mint(
            original.candidate.clone(),
            original.target.clone(),
            original.variants_sha256.clone(),
            2999,
            900,
        )
        .is_err()
    );
    assert!(
        LifecycleContext::mint(
            original.candidate.clone(),
            original.target.clone(),
            original.variants_sha256.clone(),
            5000,
            original.candidate.expires_at,
        )
        .is_err()
    );
    let mut margin = original.clone();
    margin.internal_deadline = margin.candidate.expires_at + 299;
    assert!(margin.validate().is_err());
    margin.internal_deadline += 1;
    margin.validate().unwrap();
}

#[test]
fn variant_provenance_rejects_relabels_wrong_platform_and_unbounded_builds() {
    for index in 0..3 {
        let ctx = context(index);
        let original = variants(&ctx);
        original.validate(&ctx).unwrap();
        for field in [
            "source", "target", "manifest", "version", "commit", "tree", "signing", "budget",
        ] {
            let mut changed = original.clone();
            match field {
                "source" => changed.source_commit = "9".repeat(40),
                "target" => changed.target = TARGETS[(index + 1) % 3].into(),
                "manifest" => changed.b_manifest_sha256 = "9".repeat(64),
                "version" => changed.a.version = "0.2.0".into(),
                "commit" => changed.a.commit = ctx.candidate.source_commit.clone(),
                "tree" => changed.a.tree = changed.c.tree.clone(),
                "signing" => changed.a.signing_kind = "unknown".into(),
                "budget" => changed.a.build_ms = 2_700_001,
                _ => unreachable!(),
            }
            assert!(changed.validate(&ctx).is_err(), "{field}");
        }
        let mut encoded = serde_json::to_value(original).unwrap();
        encoded["unexpected"] = true.into();
        assert!(serde_json::from_value::<Variants>(encoded).is_err());
    }
}

#[test]
fn partial_records_preserve_nulls_and_refuse_unearned_success_or_stage_order() {
    let ctx = context(0);
    let partial = LifecycleResult::new(&ctx).unwrap();
    validate_record(&partial, &ctx).unwrap();
    assert!(validate(&partial, &ctx).is_err());
    let encoded = serde_json::to_value(&partial).unwrap();
    assert!(encoded["command_wall_ms"].is_null());
    assert!(encoded["stdout_bytes"].is_null());
    let mut missing = encoded;
    missing.as_object_mut().unwrap().remove("restart_ready_ms");
    assert!(serde_json::from_value::<LifecycleResult>(missing).is_err());
    let mut invented = partial.clone();
    invented.complete = true;
    assert!(validate_record(&invented, &ctx).is_err());
    let mut skipped = partial.clone();
    skipped.stages[1].disposition = Disposition::Passed;
    skipped.stages[1].elapsed_ms = Some(1);
    assert!(validate_record(&skipped, &ctx).is_err());
    let mut cleaned_failure = partial;
    cleaned_failure.failure = Some(Failure::Inputs);
    cleaned_failure.stages[0].disposition = Disposition::Failed;
    cleaned_failure.stages[0].elapsed_ms = Some(1);
    cleaned_failure.stages[8].disposition = Disposition::Passed;
    cleaned_failure.stages[8].elapsed_ms = Some(1);
    validate_record(&cleaned_failure, &ctx).unwrap();
}

fn pins(ctx: &LifecycleContext) -> RollupPins<'_> {
    RollupPins {
        source: &ctx.candidate.source_commit,
        run_id: ctx.candidate.run_id,
        attempt: ctx.candidate.run_attempt,
        harness_sha256: &ctx.candidate.harness_sha256,
        candidate_run_id: ctx.candidate.candidate_run_id,
        candidate_attempt: ctx.candidate.candidate_run_attempt,
    }
}

fn rollup_inputs(path: &Path) {
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).unwrap();
    for (index, role) in ROLES.into_iter().enumerate() {
        let ctx = context(index);
        records::write(&path.join(format!("{role}-context.json")), &ctx).unwrap();
        records::write(
            &path.join(format!("{role}-result.json")),
            &LifecycleResult::new(&ctx).unwrap(),
        )
        .unwrap();
    }
}

#[test]
fn rollup_preserves_three_genuine_partial_results_and_stays_red() {
    let input = private_test_root();
    let output = private_test_root();
    std::fs::set_permissions(output.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    rollup_inputs(input.path());
    let ctx = context(0);
    assert!(rollup(input.path(), output.path(), "success", pins(&ctx)).is_err());
    let value: serde_json::Value =
        records::read(&output.path().join("lifecycle-rollup.json"), 65536).unwrap();
    assert_eq!(value["complete"], false);
    assert_eq!(value["records"].as_array().unwrap().len(), 3);
    for row in value["records"].as_array().unwrap() {
        assert_eq!(row["complete"], false);
        assert!(row["stdout_bytes"].is_null());
    }
    assert!(output.path().join("lifecycle-summary.md").is_file());
}

#[test]
fn rollup_rejects_each_external_pin_mismatch_before_writing_report() {
    let input = private_test_root();
    rollup_inputs(input.path());
    let ctx = context(0);
    for field in [
        "source",
        "run",
        "attempt",
        "harness",
        "candidate",
        "candidate_attempt",
    ] {
        let output = private_test_root();
        std::fs::set_permissions(output.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let mut external = pins(&ctx);
        match field {
            "source" => external.source = "9999999999999999999999999999999999999999",
            "run" => external.run_id += 1,
            "attempt" => external.attempt += 1,
            "harness" => {
                external.harness_sha256 =
                    "9999999999999999999999999999999999999999999999999999999999999999"
            }
            "candidate" => external.candidate_run_id += 1,
            "candidate_attempt" => external.candidate_attempt += 1,
            _ => unreachable!(),
        }
        assert!(
            rollup(input.path(), output.path(), "success", external).is_err(),
            "{field}"
        );
        assert!(!output.path().join("lifecycle-rollup.json").exists());
    }
}

#[test]
fn retained_file_detects_replacement_length_content_and_symlink_tampering() {
    use std::os::unix::fs::symlink;
    for mutation in ["replacement", "length", "content", "symlink"] {
        let root = private_test_root();
        let path = root.path().join("worker");
        std::fs::write(&path, b"original").unwrap();
        let held = RetainedFile::capture(&path).unwrap();
        held.verify(&path).unwrap();
        match mutation {
            "replacement" => {
                let replacement = root.path().join("replacement");
                std::fs::write(&replacement, b"original").unwrap();
                std::fs::rename(replacement, &path).unwrap();
            }
            "length" => std::fs::write(&path, b"longer original").unwrap(),
            "content" => std::fs::write(&path, b"modified").unwrap(),
            "symlink" => {
                let target = root.path().join("target");
                std::fs::rename(&path, &target).unwrap();
                symlink(target, &path).unwrap();
            }
            _ => unreachable!(),
        }
        assert!(held.verify(&path).is_err(), "{mutation}");
    }
}

// Actual download manifests and digests exercise Variants::load, without
// executing fabricated product bytes or claiming compiled variant provenance.
fn lifecycle_assets(root: &Path, version: &str, commit: &str, journal: u32) -> String {
    std::fs::create_dir_all(root).unwrap();
    let target = TARGETS[0];
    let digest = records::digest(b"x");
    let mut manifest = format!(
        "marmish-manifest 1\nversion {version}\ntarget {target}\ncommit {commit}\nsigning none notarized=no\nminimum glibc=2.39\nstate profile=2 journal={journal} engine=1 config=1 commands=1\n"
    );
    for name in native_manifest::NAMES {
        std::fs::write(root.join(format!("{name}-{version}-{target}")), b"x").unwrap();
        let mode = if name == "THIRD-PARTY-LICENSES" {
            "0444"
        } else {
            "0555"
        };
        manifest.push_str(&format!("file {digest} 1 {mode} {name}\n"));
    }
    manifest.push_str("end\n");
    std::fs::write(
        root.join(format!("marmish-{version}-{target}.manifest")),
        &manifest,
    )
    .unwrap();
    records::digest(manifest.as_bytes())
}
fn pin_variants(root: &Path, ctx: &mut LifecycleContext, variants: &Variants) {
    let bytes = records::canonical(variants).unwrap();
    ctx.variants_sha256 = records::digest(&bytes);
    std::fs::write(root.join("variants.json"), bytes).unwrap();
}

#[test]
fn variants_load_checks_real_manifest_formats_patch_and_outer_pins() {
    for mutation in ["none", "manifest", "format", "patch", "outer"] {
        let root = private_test_root();
        let candidate = root.path().join("candidate");
        let mut ctx = context(0);
        let mut v = variants(&ctx);
        v.b_manifest_sha256 = lifecycle_assets(
            &candidate,
            "0.2.0",
            &v.source_commit,
            crate::journal::JOURNAL_FORMAT.try_into().unwrap(),
        );
        ctx.candidate
            .manifests
            .insert(ROLES[0].into(), v.b_manifest_sha256.clone());
        v.a.manifest_sha256 = lifecycle_assets(
            &root.path().join("a"),
            &v.a.version,
            &v.a.commit,
            crate::journal::JOURNAL_FORMAT.try_into().unwrap(),
        );
        v.c.manifest_sha256 =
            lifecycle_assets(&root.path().join("c"), &v.c.version, &v.c.commit, 4);
        std::fs::create_dir(root.path().join("patches")).unwrap();
        for (label, variant) in [("a", &mut v.a), ("c", &mut v.c)] {
            let patch = format!("reviewed {label} patch\n");
            variant.patch_sha256 = records::digest(patch.as_bytes());
            std::fs::write(root.path().join(format!("patches/{label}.patch")), patch).unwrap();
        }
        pin_variants(root.path(), &mut ctx, &v);
        assert!(Variants::load(root.path(), &candidate, &ctx).is_ok());
        match mutation {
            "none" => continue,
            "manifest" => {
                let path = root
                    .path()
                    .join("a")
                    .join(format!("marmish-{}-{}.manifest", v.a.version, v.target));
                let text = std::fs::read_to_string(&path).unwrap();
                std::fs::write(path, text.replace("engine=1", "engine=2")).unwrap();
            }
            "format" => {
                // Re-pin the forged manifest and outer record: the format
                // relation itself must reject A's lower journal format.
                v.a.manifest_sha256 =
                    lifecycle_assets(&root.path().join("a"), &v.a.version, &v.a.commit, 4);
                pin_variants(root.path(), &mut ctx, &v);
            }
            "patch" => {
                std::fs::write(root.path().join("patches/a.patch"), b"unreviewed patch").unwrap()
            }
            "outer" => ctx.variants_sha256 = "0".repeat(64),
            _ => unreachable!(),
        }
        assert!(
            Variants::load(root.path(), &candidate, &ctx).is_err(),
            "{mutation}"
        );
    }
}

fn child_environment(root: &Path) -> super::super::process::Environment {
    super::super::process::Environment {
        home: root.into(),
        temporary: root.into(),
        utilities: root.into(),
    }
}

#[tokio::test]
async fn execute_preserves_binary_drains_and_refuses_each_stream_overflow() {
    let root = private_test_root();
    let environment = child_environment(root.path());
    let (_tx, rx) = tokio::sync::watch::channel(false);
    let cancel = Cancellation(rx);
    for stderr in [false, true] {
        for length in [524288, 524289] {
            let bytes: Vec<u8> = (0..length).map(|n| (n % 251) as u8).collect();
            let script = if stderr {
                "exec /bin/cat >&2"
            } else {
                "exec /bin/cat"
            };
            let result = execute(
                Path::new("/bin/sh"),
                &["-c".into(), script.into()],
                root.path(),
                &environment,
                bytes.clone(),
                tokio::time::Instant::now() + std::time::Duration::from_secs(5),
                &cancel,
            )
            .await;
            if length > 524288 {
                assert!(result.is_err(), "overflow stderr={stderr}");
            } else {
                let output = result.unwrap();
                assert!(output.status.success());
                if stderr {
                    assert_eq!(output.stderr, bytes);
                    assert!(output.stdout.is_empty());
                } else {
                    assert_eq!(output.stdout, bytes);
                    assert!(output.stderr.is_empty());
                }
            }
        }
    }
}

async fn wait_for_pid(path: &Path) -> i32 {
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        loop {
            if let Ok(bytes) = std::fs::read_to_string(path)
                && let Ok(pid) = bytes.trim().parse::<i32>()
            {
                return pid;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap()
}

#[tokio::test]
async fn execute_timeout_and_cancellation_reap_the_owned_process_group() {
    for cancellation in [false, true] {
        let root = private_test_root();
        let env = child_environment(root.path());
        let (tx, rx) = tokio::sync::watch::channel(false);
        let cancel = Cancellation(rx);
        let pid_path = root.path().join("pid");
        let args = ["-c".into(), "echo $$ > pid; exec /bin/sleep 30".into()];
        let run = execute(
            Path::new("/bin/sh"),
            &args,
            root.path(),
            &env,
            Vec::new(),
            tokio::time::Instant::now() + std::time::Duration::from_millis(500),
            &cancel,
        );
        let (result, pid) = tokio::join!(run, async {
            let pid = wait_for_pid(&pid_path).await;
            if cancellation {
                tx.send(true).unwrap();
            }
            pid
        });
        if cancellation {
            assert!(matches!(
                result,
                Err(Error::Invalid("lifecycle_interrupted"))
            ));
        } else {
            assert!(matches!(result, Err(Error::Timeout)));
        }
        assert_eq!(unsafe { libc::kill(pid, 0) }, -1);
        assert_eq!(
            std::io::Error::last_os_error().raw_os_error(),
            Some(libc::ESRCH)
        );
        assert_eq!(unsafe { libc::kill(-pid, 0) }, -1);
        assert_eq!(
            std::io::Error::last_os_error().raw_os_error(),
            Some(libc::ESRCH)
        );
    }
}

#[test]
fn database_inventory_rejects_existing_bytes_and_unrelated_file_changes() {
    for mutation in ["main", "wal", "config", "extra"] {
        let root = private_test_root();
        std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        directory(&root.path().join("commands")).unwrap();
        for name in [
            "mdk.db",
            "mdk.db-wal",
            "journal.db",
            "commands/engine.sqlite3",
            "config.json",
        ] {
            let path = root.path().join(name);
            std::fs::write(&path, b"retained").unwrap();
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).unwrap();
        }
        let inventory = DatabaseInventory::capture(root.path()).unwrap();
        inventory.verify().unwrap();
        let name = match mutation {
            "main" => "mdk.db",
            "wal" => "mdk.db-wal",
            "config" => "config.json",
            "extra" => "unexpected",
            _ => unreachable!(),
        };
        let path = root.path().join(name);
        std::fs::write(&path, b"mutation").unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert!(inventory.verify().is_err(), "{mutation}");
    }
}

#[tokio::test]
async fn cancellation_latches_red_and_pending_bindings_block_cleanup() {
    for operation_ok in [false, true] {
        for cleanup_ok in [false, true] {
            for cancelled in [false, true] {
                assert_eq!(
                    qualification_complete(operation_ok, cleanup_ok, cancelled),
                    operation_ok && cleanup_ok && !cancelled
                );
            }
        }
    }
    assert!(
        !cleanup_allowed(true, 1),
        "a known row cannot erase a pending binding"
    );
    assert!(!cleanup_allowed(true, 0));
    assert!(
        !cleanup_allowed(false, 0),
        "empty evidence cannot prove ownership"
    );
    assert!(cleanup_allowed(false, 1));
    let (tx, rx) = tokio::sync::watch::channel(false);
    let cancellation = Cancellation(rx);
    cancellation.check().unwrap();
    tx.send(true).unwrap();
    assert!(matches!(
        cancellation.check(),
        Err(Error::Invalid("lifecycle_interrupted"))
    ));
    tokio::time::timeout(
        std::time::Duration::from_millis(100),
        cancellation.cancelled(),
    )
    .await
    .unwrap();
    assert!(!qualification_complete(
        true,
        true,
        cancellation.check().is_err()
    ));
}

#[tokio::test]
async fn known_context_input_failures_write_finite_red_records_without_launching() {
    for failure in ["deadline", "workspace", "assets"] {
        let root = private_test_root();
        std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let workspace = root.path().join("workspace");
        directory(&workspace).unwrap();
        let mut ctx = context(0);
        let now = super::super::now().unwrap();
        ctx.candidate.created_at = if failure == "deadline" {
            now - 400
        } else {
            now
        };
        ctx.candidate.expires_at = ctx.candidate.created_at + 1800;
        ctx.internal_deadline = ctx.candidate.expires_at + 300;
        ctx.validate().unwrap();
        let context_path = root.path().join("context.json");
        records::write(&context_path, &ctx).unwrap();
        if failure == "workspace" {
            std::fs::write(workspace.join("existing"), b"retain").unwrap();
        }
        let output_path = root.path().join("result.json");
        let absent = root.path().join("absent");
        assert!(
            run_cli(
                &context_path,
                &workspace,
                &output_path,
                &absent,
                &absent,
                &absent,
                &absent
            )
            .await
            .is_err()
        );
        let result: LifecycleResult = records::read(&output_path, 65536).unwrap();
        validate_record(&result, &ctx).unwrap();
        assert!(!result.complete);
        assert!(result.failure == Some(Failure::Inputs));
        assert!(result.stages[0].disposition == Disposition::Failed);
        assert!(
            result.stages[1..]
                .iter()
                .all(|stage| stage.disposition == Disposition::Unreached)
        );
        assert!(
            !result.engine_cleanup_complete
                && !result.server_stop_complete
                && !result.client_stop_complete
        );
        assert!(!workspace.join("owned").exists());
        if failure == "workspace" {
            assert_eq!(
                std::fs::read(workspace.join("existing")).unwrap(),
                b"retain"
            );
        }
        let encoded = serde_json::to_value(&result).unwrap();
        let mut missing_failure = encoded.clone();
        missing_failure.as_object_mut().unwrap().remove("failure");
        assert!(serde_json::from_value::<LifecycleResult>(missing_failure).is_err());
        let mut invalid_failure = encoded;
        invalid_failure["failure"] = "provider_raw_secret_error".into();
        assert!(serde_json::from_value::<LifecycleResult>(invalid_failure).is_err());
    }
}

#[test]
fn stage_budget_uses_original_end_and_accepts_exact_platform_boundaries() {
    use std::time::Duration;
    use tokio::time::Instant;

    for provider_required in [false, true] {
        for stage in STAGES {
            let seconds = match (stage, provider_required) {
                (Stage::RollbackA, false) => Some(600),
                (Stage::RollbackA, true) => Some(1140),
                (Stage::RejectC, false) => Some(480),
                (Stage::RejectC, true) => Some(1020),
                (Stage::Provider, true) => Some(840),
                _ => None,
            };
            assert_eq!(
                stage_budget(stage, provider_required),
                seconds.map(Duration::from_secs)
            );
            let now = Instant::now();
            let Some(seconds) = seconds else {
                require_stage_budget(stage, provider_required, now, now).unwrap();
                continue;
            };
            let end = now + Duration::from_secs(seconds);
            // Equality is admissible. The stage allocation already includes
            // cleanup; subtracting the separate work reserve again is wrong.
            require_stage_budget(stage, provider_required, end, now).unwrap();
            require_stage_budget(stage, provider_required, end + Duration::from_nanos(1), now)
                .unwrap();
            assert!(matches!(
                require_stage_budget(stage, provider_required, end - Duration::from_nanos(1), now),
                Err(Error::Timeout)
            ));
            let work = end - Duration::from_secs(300);
            assert!(matches!(
                require_stage_budget(stage, provider_required, work, now),
                Err(Error::Timeout)
            ));
            assert!(matches!(
                require_stage_budget(stage, provider_required, now, now + Duration::from_nanos(1)),
                Err(Error::Timeout)
            ));
        }
    }
}
