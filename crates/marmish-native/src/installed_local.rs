//! Local installed qualification routing and closed three-platform aggregation.
use super::{autoexit, benchmark, policy, records, recovery};
use crate::{Error, Result};
use records::{Context, IdentityReady, ROLES};
use serde::Serialize;
use std::{collections::BTreeMap, fs, path::Path};

fn bad() -> Error {
    Error::Invalid("installed_local")
}

pub(super) async fn run(
    cohort: &str,
    context: &Path,
    workspace: &Path,
    output: &Path,
) -> Result<()> {
    let ctx: Context = records::read(context, 65536)?;
    ctx.validate()?;
    ctx.require_cohort(cohort)?;
    if cohort == "matrix" || super::now()? >= ctx.expires_at {
        return Err(bad());
    }
    let identity = records::own_identity(workspace, &ctx)?;
    let directory = workspace.join("output");
    if output != directory.join(format!("{}-{cohort}-result.json", identity.role)) {
        return Err(bad());
    }
    match cohort {
        "autoexit" | "uptime" => {
            let result = autoexit::run(&ctx, workspace, &directory).await?;
            // The final result is written after the component's private cleanup.
            records::write(output, &result)?;
            autoexit::validate(&result, &ctx, &identity.role, &identity)
        }
        "policy" => {
            let result = policy::run(&ctx, workspace, &directory).await?;
            records::write(output, &result)?;
            policy::validate(&result, &ctx, &identity.role, &identity)
        }
        "benchmark" => benchmark::run(&ctx, workspace, output).await,
        "recovery" => {
            recovery::run(&ctx, workspace, output).await?;
            let result: recovery::RecoveryResult = records::read(output, records::RECORD_LIMIT)?;
            recovery::validate(&result, &ctx, &identity)
        }
        _ => Err(bad()),
    }
}

#[derive(Serialize)]
struct Rollup {
    schema: u8,
    scope: String,
    context_sha256: String,
    cohort: String,
    repository: String,
    source_commit: String,
    candidate_run_id: u64,
    candidate_run_attempt: u64,
    version: String,
    platform_results: BTreeMap<String, serde_json::Value>,
    complete: bool,
    pending_gates: Vec<&'static str>,
}

pub(super) fn rollup(
    cohort: &str,
    context: &Path,
    directory: &Path,
    job_result: &str,
    output: &Path,
) -> Result<()> {
    let ctx: Context = records::read(context, 65536)?;
    ctx.validate()?;
    ctx.require_cohort(cohort)?;
    if cohort == "matrix"
        || !matches!(
            cohort,
            "autoexit" | "uptime" | "benchmark" | "recovery" | "policy"
        )
    {
        return Err(bad());
    }
    let expected = ROLES
        .into_iter()
        .flat_map(|r| {
            [
                format!("{r}-identity-ready.json"),
                format!("{r}-{cohort}-result.json"),
            ]
        })
        .collect::<std::collections::BTreeSet<_>>();
    let actual = fs::read_dir(directory)?
        .map(|entry| entry?.file_name().into_string().map_err(|_| bad()))
        .collect::<Result<std::collections::BTreeSet<_>>>()?;
    let mut complete = actual == expected && job_result == "success";
    let mut platforms = BTreeMap::new();
    let mut markdown = format!(
        "# Installed {cohort} qualification\n\nSource `{}`; candidate {}/{}.\n\n",
        ctx.source_commit, ctx.candidate_run_id, ctx.candidate_run_attempt
    );
    for role in ROLES {
        let checked = (|| -> Result<(serde_json::Value, bool, String)> {
            let identity: IdentityReady = records::read(
                &directory.join(format!("{role}-identity-ready.json")),
                65536,
            )?;
            identity.validate(&ctx, role)?;
            let original = records::read_bytes(
                &directory.join(format!("{role}-identity-ready.json")),
                65536,
            )?;
            if original != records::canonical(&identity)? {
                return Err(bad());
            }
            let path = directory.join(format!("{role}-{cohort}-result.json"));
            if cohort == "policy" {
                let result: policy::PolicyResult = records::read(&path, records::RECORD_LIMIT)?;
                policy::validate_record(&result, &ctx, role, &identity)?;
                let passed = result.is_complete();
                Ok((
                    serde_json::to_value(&result).map_err(|_| bad())?,
                    passed,
                    format!("- {role}: {}\n", if passed { "passed" } else { "failed" }),
                ))
            } else if cohort == "recovery" {
                let result: recovery::RecoveryResult = records::read(&path, records::RECORD_LIMIT)?;
                recovery::validate_record(&result, &ctx, &identity)?;
                let passed = result.is_complete();
                Ok((
                    serde_json::to_value(&result).map_err(|_| bad())?,
                    passed,
                    recovery::markdown_summary(&result)?,
                ))
            } else if cohort == "benchmark" {
                let result: benchmark::BenchmarkResult =
                    records::read(&path, records::RECORD_LIMIT)?;
                benchmark::validate_record(&result, &ctx, &identity)?;
                Ok((
                    serde_json::to_value(&result).map_err(|_| bad())?,
                    result.is_complete(),
                    benchmark::markdown_summary(&result)?,
                ))
            } else {
                let result: autoexit::CohortResult = records::read(&path, records::RECORD_LIMIT)?;
                autoexit::validate_record(&result, &ctx, role, &identity)?;
                let passed = result.is_complete();
                let value = serde_json::to_value(result).map_err(|_| bad())?;
                Ok((
                    value,
                    passed,
                    format!("- {role}: {}\n", if passed { "passed" } else { "failed" }),
                ))
            }
        })();
        match checked {
            Ok((record, passed, text)) => {
                complete &= passed;
                platforms.insert(role.to_owned(), record);
                markdown.push_str(&text);
            }
            Err(_) => {
                complete = false;
                platforms.insert(role.to_owned(), serde_json::json!({"role":role,"complete":false,"category":"missing_or_invalid"}));
                markdown.push_str(&format!("- {role}: missing or invalid; unqualified\n"));
            }
        }
    }
    let pending_gates = [
        "matrix",
        "autoexit",
        "uptime",
        "benchmark",
        "recovery",
        "policy",
        "default-auto keychains",
        "asset publication",
        "HTTPS GitHub",
        "HTTPS Iris",
    ]
    .into_iter()
    .filter(|name| *name != cohort)
    .collect();
    let report = Rollup {
        schema: 1,
        scope: format!("installed-{cohort}"),
        context_sha256: ctx.digest()?,
        cohort: cohort.into(),
        repository: ctx.repository,
        source_commit: ctx.source_commit,
        candidate_run_id: ctx.candidate_run_id,
        candidate_run_attempt: ctx.candidate_run_attempt,
        version: ctx.version,
        platform_results: platforms,
        complete,
        pending_gates,
    };
    markdown.push_str(if complete {
        "\nCohort passed. Separate release gates remain required.\n"
    } else {
        "\nCohort failed; no release qualification.\n"
    });
    records::write(&output.join("cohort-rollup.json"), &report)?;
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(output.join("cohort-rollup.md"))?;
    file.write_all(markdown.as_bytes())?;
    file.sync_all()?;
    if complete { Ok(()) } else { Err(bad()) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn private_dir() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o700)).unwrap();
        dir
    }

    #[test]
    fn missing_records_produce_red_typed_three_platform_rollup() {
        for cohort in ["autoexit", "uptime", "benchmark", "recovery", "policy"] {
            let dir = private_dir();
            let mut ctx = records::tests::ctx();
            ctx.cohort = cohort.into();
            ctx.expires_at = ctx.created_at + 3300;
            let context = dir.path().join("context.json");
            records::write(&context, &ctx).unwrap();
            let input = dir.path().join("input");
            let output = dir.path().join("output");
            fs::create_dir(&input).unwrap();
            fs::create_dir(&output).unwrap();
            fs::set_permissions(&input, fs::Permissions::from_mode(0o700)).unwrap();
            fs::set_permissions(&output, fs::Permissions::from_mode(0o700)).unwrap();
            assert!(rollup(cohort, &context, &input, "failure", &output).is_err());
            let result: serde_json::Value =
                records::read(&output.join("cohort-rollup.json"), 65536).unwrap();
            assert_eq!(result["complete"], false);
            assert_eq!(result["context_sha256"], ctx.digest().unwrap());
            assert_eq!(result["platform_results"].as_object().unwrap().len(), 3);
            for role in ROLES {
                assert_eq!(result["platform_results"][role]["complete"], false);
                assert_eq!(
                    result["platform_results"][role]["category"],
                    "missing_or_invalid"
                );
            }
            assert!(output.join("cohort-rollup.md").is_file());
            // A second publication must never overwrite immutable red evidence.
            assert!(rollup(cohort, &context, &input, "success", &output).is_err());
        }
    }

    #[test]
    fn genuine_failed_benchmark_survives_rollup_without_qualification() {
        let dir = private_dir();
        let (ctx, identity, failed) = benchmark::tests::failed_record();
        let context = dir.path().join("context.json");
        records::write(&context, &ctx).unwrap();
        let input = dir.path().join("input");
        let output = dir.path().join("output");
        for path in [&input, &output] {
            fs::create_dir(path).unwrap();
            fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
        }
        records::write(
            &input.join(format!("{}-identity-ready.json", identity.role)),
            &identity,
        )
        .unwrap();
        records::write(
            &input.join(format!("{}-benchmark-result.json", identity.role)),
            &failed,
        )
        .unwrap();
        assert!(rollup("benchmark", &context, &input, "failure", &output).is_err());
        let report: serde_json::Value =
            records::read(&output.join("cohort-rollup.json"), 65536).unwrap();
        assert_eq!(report["complete"], false);
        assert_eq!(
            report["platform_results"][&identity.role],
            serde_json::to_value(&failed).unwrap()
        );
        let summary = fs::read_to_string(output.join("cohort-rollup.md")).unwrap();
        assert!(summary.contains("Warm n=0"));
        assert!(summary.contains("unqualified | null | null"));
    }

    #[test]
    fn all_success_jobs_cannot_qualify_three_bound_failed_records() {
        for cohort in ["benchmark", "recovery", "policy"] {
            let dir = private_dir();
            let input = dir.path().join("input");
            let output = dir.path().join("output");
            for path in [&input, &output] {
                fs::create_dir(path).unwrap();
                fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
            }
            let context = dir.path().join("context.json");
            let mut expected = BTreeMap::new();
            let mut bound_context = None;
            for role in ROLES {
                let (ctx, identity, value) = if cohort == "benchmark" {
                    let (ctx, identity, result) = benchmark::tests::failed_record_for(role);
                    (ctx, identity, serde_json::to_value(result).unwrap())
                } else if cohort == "policy" {
                    let (ctx, identity, result) = policy::tests::failed_record_for(role);
                    (ctx, identity, serde_json::to_value(result).unwrap())
                } else {
                    let (ctx, identity, result) = recovery::tests::failed_record(role);
                    (ctx, identity, serde_json::to_value(result).unwrap())
                };
                if let Some(ref previous) = bound_context {
                    assert_eq!(&records::canonical(&ctx).unwrap(), previous);
                } else {
                    records::write(&context, &ctx).unwrap();
                    bound_context = Some(records::canonical(&ctx).unwrap());
                }
                records::write(
                    &input.join(format!("{role}-identity-ready.json")),
                    &identity,
                )
                .unwrap();
                records::write(&input.join(format!("{role}-{cohort}-result.json")), &value)
                    .unwrap();
                expected.insert(role, value);
            }
            assert_eq!(fs::read_dir(&input).unwrap().count(), 6);
            assert!(rollup(cohort, &context, &input, "success", &output).is_err());
            let report: serde_json::Value =
                records::read(&output.join("cohort-rollup.json"), records::RECORD_LIMIT).unwrap();
            assert_eq!(report["complete"], false);
            for role in ROLES {
                assert_eq!(report["platform_results"][role], expected[role]);
            }
        }
    }

    #[test]
    fn wrong_cohort_is_refused_before_output_publication() {
        let dir = private_dir();
        let context = dir.path().join("context.json");
        records::write(&context, &records::tests::ctx()).unwrap();
        assert!(rollup("autoexit", &context, dir.path(), "success", dir.path()).is_err());
        assert!(!dir.path().join("cohort-rollup.json").exists());
    }
}
