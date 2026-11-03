// Included in native_install::tests to reuse its owned asset/transaction fixture.
fn legacy_fixture(f: &Fixture, suffix: &str) -> (InstallOptions, PathBuf, Vec<Vec<u8>>) {
    let root = f.root.path().join(format!("legacy {suffix}"));
    fs::create_dir(&root).unwrap();
    fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
    let mut options = f.options(InstallAction::Install {
        source_dir: f.source.clone(),
        manifest: f.manifest.clone(),
        manifest_sha256: f.hash,
    });
    options.bin_dir = root.join("bin");
    fs::create_dir(&options.bin_dir).unwrap();
    let record = legacy::Legacy {
        root: root.clone(),
        version: "0.1.0-experimental.2".into(),
        target: env!("MARMISH_BUILD_TARGET").into(),
        launchers: LAUNCHERS.iter().map(|n| (*n).into()).collect(),
    };
    let runtime = record.version_root();
    fs::create_dir_all(runtime.join("bin")).unwrap();
    fs::write(runtime.join("runtime-sentinel"), b"retain old runtime").unwrap();
    fs::create_dir(root.join("v2")).unwrap();
    fs::write(root.join("v2/private-sentinel"), b"retain private state").unwrap();
    fs::write(root.join(".marmish-install.json"),serde_json::to_vec(&serde_json::json!({
        "format":1,"version":record.version,"target":record.target,"launcher_dir":options.bin_dir,
    })).unwrap()).unwrap();
    fs::set_permissions(
        root.join(".marmish-install.json"),
        fs::Permissions::from_mode(0o600),
    )
    .unwrap();
    fs::write(root.join(".marmish-install.lock"), b"").unwrap();
    fs::set_permissions(
        root.join(".marmish-install.lock"),
        fs::Permissions::from_mode(0o600),
    )
    .unwrap();
    std::os::unix::fs::symlink(
        Path::new("versions").join(runtime.file_name().unwrap()),
        root.join("current"),
    )
    .unwrap();
    let mut originals = Vec::new();
    for name in LAUNCHERS {
        // Captured v0.1 launcher_bytes template. The fixture root always has a
        // space, so Python shlex.quote always uses this exact apostrophe escape.
        let target = runtime.join("bin").join(name);
        let quoted = target.to_str().unwrap().replace('\'', "'\"'\"'");
        let body = format!("#!/bin/sh\n# marmish installer format 1\nexec '{quoted}' \"$@\"\n")
            .into_bytes();
        fs::write(options.bin_dir.join(name), &body).unwrap();
        fs::set_permissions(
            options.bin_dir.join(name),
            fs::Permissions::from_mode(0o755),
        )
        .unwrap();
        originals.push(body);
    }
    (options, root, originals)
}
fn legacy_install(f: &Fixture, bin: &Path) -> Result<InstallReport> {
    let mut options = f.options(InstallAction::Install {
        source_dir: f.source.clone(),
        manifest: f.manifest.clone(),
        manifest_sha256: f.hash,
    });
    options.bin_dir = bin.to_owned();
    run(options)
}
fn legacy_status(f: &Fixture, bin: &Path) -> Result<InstallReport> {
    let mut options = f.options(InstallAction::Status);
    options.bin_dir = bin.to_owned();
    run(options)
}
#[test]
fn legacy_upgrade_retains_exact_wrappers_runtime_and_nested_state() {
    let f = Fixture::new();
    let (o, root, originals) = legacy_fixture(&f, "old prefix ' $;`quoted`");
    let bin = o.bin_dir.clone();
    assert!(run(o).unwrap().active);
    let backups: Vec<_> = fs::read_dir(&bin)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| {
            p.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with(".marmish-legacy-")
        })
        .collect();
    assert_eq!(backups.len(), 2);
    for (i, name) in LAUNCHERS.iter().enumerate() {
        let backup = backups
            .iter()
            .find(|p| p.file_name().unwrap().to_string_lossy().ends_with(name))
            .unwrap();
        assert_eq!(fs::read(backup).unwrap(), originals[i]);
        assert!(
            fs::symlink_metadata(bin.join(name))
                .unwrap()
                .file_type()
                .is_symlink()
        );
    }
    assert_eq!(
        fs::read(root.join("v2/private-sentinel")).unwrap(),
        b"retain private state"
    );
    assert_eq!(
        fs::read(root.join("current/runtime-sentinel")).unwrap(),
        b"retain old runtime"
    );
    assert!(legacy_install(&f, &bin).unwrap().active);
    let mut select = f.options(InstallAction::Select {
        version: env!("CARGO_PKG_VERSION").into(),
    });
    select.bin_dir = bin.clone();
    assert!(run(select).unwrap().active);
    let mut uninstall = f.options(InstallAction::Uninstall);
    uninstall.bin_dir = bin.clone();
    assert!(!run(uninstall).unwrap().active);
    assert!(backups.iter().all(|p| p.is_file()));
}
#[test]
fn legacy_every_transaction_boundary_recovers_without_deleting_originals() {
    for checkpoint in [
        "transaction",
        "legacy_marmish",
        "legacy_marmishd",
        "copy",
        "version",
        "selector",
        "commit",
    ] {
        let f = Fixture::new();
        let (o, root, originals) = legacy_fixture(&f, "old");
        let bin = o.bin_dir.clone();
        FAULT.with(|v| *v.borrow_mut() = Some(checkpoint));
        assert!(run(o).is_err(), "{checkpoint}");
        FAULT.with(|v| *v.borrow_mut() = None);
        // Recovery relies on the transaction, not a now-absent legacy tree.
        fs::remove_file(root.join(".marmish-install.json")).unwrap();
        fs::remove_file(root.join(".marmish-install.lock")).unwrap();
        let report = legacy_status(&f, &bin).unwrap();
        assert_eq!(report.active, checkpoint == "commit", "{checkpoint}");
        for (i, name) in LAUNCHERS.iter().enumerate() {
            if checkpoint != "commit" {
                assert_eq!(
                    fs::read(bin.join(name)).unwrap(),
                    originals[i],
                    "{checkpoint}"
                );
            }
        }
        assert!(!f.prefix().join(TRANSACTION).exists());
    }
}
#[test]
fn legacy_partial_pair_is_upgraded_and_foreign_pair_is_never_partially_moved() {
    let f = Fixture::new();
    let (o, _, _) = legacy_fixture(&f, "old");
    fs::remove_file(o.bin_dir.join("marmishd")).unwrap();
    assert!(run(o).unwrap().active);
    for foreign in ["regular", "symlink", "dangling", "hardlink"] {
        let f = Fixture::new();
        let (o, _, originals) = legacy_fixture(&f, "old");
        let bin = o.bin_dir.clone();
        fs::remove_file(bin.join("marmishd")).unwrap();
        match foreign {
            "symlink" => std::os::unix::fs::symlink(&f.manifest, bin.join("marmishd")).unwrap(),
            "dangling" => {
                std::os::unix::fs::symlink("unrelated-missing", bin.join("marmishd")).unwrap()
            }
            "hardlink" => fs::hard_link(bin.join("marmish"), bin.join("marmishd")).unwrap(),
            _ => fs::write(bin.join("marmishd"), b"unrelated").unwrap(),
        }
        let before = fs::symlink_metadata(bin.join("marmishd")).unwrap();
        assert!(
            matches!(run(o), Err(Error::Invalid("unrelated_launcher"))),
            "{foreign}"
        );
        assert_eq!(fs::read(bin.join("marmish")).unwrap(), originals[0]);
        assert_eq!(
            fs::symlink_metadata(bin.join("marmishd")).unwrap().ino(),
            before.ino()
        );
        assert!(!f.prefix().join(TRANSACTION).exists());
    }
}
#[test]
fn legacy_bad_marker_selector_permissions_and_lock_are_preserved() {
    for defect in [
        "version",
        "target",
        "unknown",
        "selector",
        "mode",
        "missing_lock",
        "held_lock",
        "overlap",
    ] {
        let f = Fixture::new();
        let (mut o, root, originals) = legacy_fixture(&f, "old");
        let bin = o.bin_dir.clone();
        let mut guard = None;
        match defect {
            "version" | "target" | "unknown" => {
                let p = root.join(".marmish-install.json");
                let mut v: serde_json::Value =
                    serde_json::from_slice(&fs::read(&p).unwrap()).unwrap();
                if defect == "version" {
                    v["version"] = "0.1.0-experimental.02".into();
                } else if defect == "target" {
                    v["target"] = "other-target".into();
                } else {
                    v["extra"] = true.into();
                }
                fs::write(p, serde_json::to_vec(&v).unwrap()).unwrap();
            }
            "selector" => {
                fs::remove_file(root.join("current")).unwrap();
                std::os::unix::fs::symlink("other", root.join("current")).unwrap();
            }
            "mode" => fs::set_permissions(bin.join("marmishd"), fs::Permissions::from_mode(0o777))
                .unwrap(),
            "missing_lock" => fs::remove_file(root.join(".marmish-install.lock")).unwrap(),
            "held_lock" => {
                let lock = fs::OpenOptions::new()
                    .read(true)
                    .write(true)
                    .open(root.join(".marmish-install.lock"))
                    .unwrap();
                lock.try_lock_exclusive().unwrap();
                guard = Some(lock);
            }
            "overlap" => o.prefix = root.join("native"),
            _ => unreachable!(),
        }
        assert!(run(o).is_err(), "{defect}");
        drop(guard);
        for (i, name) in LAUNCHERS.iter().enumerate() {
            assert_eq!(fs::read(bin.join(name)).unwrap(), originals[i], "{defect}");
        }
    }
}
#[test]
fn legacy_recovery_rejects_altered_backup_and_foreign_replacement_without_mutation() {
    for altered in [false, true] {
        let f = Fixture::new();
        let (o, _, _) = legacy_fixture(&f, "old");
        let bin = o.bin_dir.clone();
        FAULT.with(|v| *v.borrow_mut() = Some("legacy_marmish"));
        assert!(run(o).is_err());
        FAULT.with(|v| *v.borrow_mut() = None);
        let tx: Transaction = json(&f.prefix().join(TRANSACTION), 0o600, 8192).unwrap();
        let p = if altered {
            tx.legacy
                .as_ref()
                .unwrap()
                .backup(&read_receipt(&f.prefix()).unwrap(), &tx, "marmish")
        } else {
            bin.join("marmish")
        };
        fs::write(&p, b"unrelated replacement").unwrap();
        assert!(legacy_status(&f, &bin).is_err());
        assert_eq!(fs::read(p).unwrap(), b"unrelated replacement");
        assert!(f.prefix().join(TRANSACTION).exists());
    }
}
#[test]
fn legacy_transaction_size_is_bounded_before_any_tempfile() {
    let f = Fixture::new();
    fs::create_dir(f.prefix()).unwrap();
    let ctx = Anchors::default();
    let v = "a".repeat(8189);
    assert!(atomic(&ctx, &f.prefix(), TRANSACTION, &v).is_ok());
    assert_eq!(
        fs::metadata(f.prefix().join(TRANSACTION)).unwrap().len(),
        8192
    );
    assert_eq!(
        json::<String>(&f.prefix().join(TRANSACTION), 0o600, 8192).unwrap(),
        v
    );
    fs::remove_file(f.prefix().join(TRANSACTION)).unwrap();
    assert!(atomic(&ctx, &f.prefix(), TRANSACTION, &"a".repeat(8190)).is_err());
    assert_eq!(fs::read_dir(f.prefix()).unwrap().count(), 0);
}
#[test]
fn legacy_wrapper_template_and_path_validation_match_published_contract() {
    use std::os::unix::ffi::OsStringExt;
    let mut v = legacy::Legacy {
        root: "/home/example/old".into(),
        version: "0.1.0-experimental.1".into(),
        target: env!("MARMISH_BUILD_TARGET").into(),
        launchers: vec!["marmish".into()],
    };
    assert_eq!(v.wrapper("marmish").unwrap(),format!("#!/bin/sh\n# marmish installer format 1\nexec /home/example/old/versions/marmish-0.1.0-experimental.1-{}/bin/marmish \"$@\"\n",env!("MARMISH_BUILD_TARGET")).as_bytes());
    v.root = PathBuf::from(std::ffi::OsString::from_vec(b"/home/invalid-\xff".to_vec()));
    assert!(v.validate().is_err());
    v.root = "/home/example/../old".into();
    assert!(v.validate().is_err());
    v.root = PathBuf::from(format!("/{}", "a".repeat(2048)));
    assert!(v.validate().is_err());
}
#[test]
fn legacy_default_home_upgrade_preserves_nested_v2_state_in_separate_process() {
    const CHILD: &str = "MARMISH_LEGACY_DEFAULT_CHILD";
    if std::env::var_os(CHILD).is_some() {
        let f = Fixture::new();
        let (mut o, old, _) = legacy_fixture(&f, "old");
        let home = PathBuf::from(std::env::var_os("HOME").unwrap());
        let root = home.join(".local/share/marmish");
        let bin = home.join(".local/bin");
        fs::create_dir_all(root.parent().unwrap()).unwrap();
        fs::create_dir_all(&bin).unwrap();
        fs::rename(&old, &root).unwrap();
        for name in LAUNCHERS {
            fs::remove_file(root.join("bin").join(name)).unwrap();
            let target = root.join("current");
            let relative = fs::read_link(target).unwrap();
            let executable = root.join(relative).join("bin").join(name);
            // HOME fixture has a space; exact historical quoted wrapper.
            let quoted = executable.to_str().unwrap().replace('\'', "'\"'\"'");
            fs::write(
                bin.join(name),
                format!("#!/bin/sh\n# marmish installer format 1\nexec '{quoted}' \"$@\"\n"),
            )
            .unwrap();
            fs::set_permissions(bin.join(name), fs::Permissions::from_mode(0o755)).unwrap();
        }
        let marker = root.join(".marmish-install.json");
        let mut m: serde_json::Value = serde_json::from_slice(&fs::read(&marker).unwrap()).unwrap();
        m["launcher_dir"] = bin.to_str().unwrap().into();
        fs::write(marker, serde_json::to_vec(&m).unwrap()).unwrap();
        o.bin_dir = bin;
        assert!(run(o).unwrap().active);
        assert_eq!(
            fs::read(root.join("v2/private-sentinel")).unwrap(),
            b"retain private state"
        );
        return;
    }
    let private = tempfile::Builder::new()
        .prefix("marmish legacy home ")
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir_in(std::env::var_os("HOME").unwrap())
        .unwrap();
    let home = private.path().canonicalize().unwrap();
    let status=std::process::Command::new(std::env::current_exe().unwrap()).args(["--exact","native_install::tests::legacy_default_home_upgrade_preserves_nested_v2_state_in_separate_process","--nocapture"]).env(CHILD,"1").env("HOME",home).env_remove("XDG_DATA_HOME").status().unwrap();
    assert!(status.success());
}
fn legacy_diagnostic_path(stdout: &str) -> Option<&str> {
    const MARKER: &str = "EXPECTED_PATH=";
    if stdout.len() > 65_536 {
        return None;
    }
    let (_, rest) = stdout.split_once(MARKER)?;
    if rest.contains(MARKER) {
        return None;
    }
    let quoted = rest.split('\n').next()?;
    let path = serde_json::from_str::<String>(quoted).ok()?;
    (serde_json::to_string(&path).ok()? == quoted).then_some(quoted)
}
#[test]
fn legacy_diagnostic_marker_accepts_libtest_prefix_and_rejects_ambiguous_output() {
    let quoted = serde_json::to_string("/owned/quoted \"; $`path`\n/marmishd").unwrap();
    for prefix in ["", "test native_install::tests::child ... "] {
        let stdout = format!("{prefix}EXPECTED_PATH={quoted}\nok\n");
        assert_eq!(legacy_diagnostic_path(&stdout), Some(quoted.as_str()));
    }
    for stdout in [
        "no marker".to_owned(),
        "EXPECTED_PATH=".to_owned(),
        "EXPECTED_PATH=123\n".to_owned(),
        format!("EXPECTED_PATH={quoted}\r\n"),
        format!("EXPECTED_PATH={quoted} \n"),
        format!("EXPECTED_PATH={quoted} EXPECTED_PATH={quoted}\n"),
        format!("EXPECTED_PATH={quoted}\nEXPECTED_PATH={quoted}\n"),
        format!("{}EXPECTED_PATH={quoted}", "x".repeat(65_536)),
    ] {
        assert_eq!(legacy_diagnostic_path(&stdout), None, "{stdout:?}");
    }
}
#[test]
fn legacy_foreign_launcher_diagnostic_quotes_exact_path_in_separate_process() {
    const CHILD: &str = "MARMISH_LEGACY_DIAGNOSTIC_CHILD";
    if std::env::var_os(CHILD).is_some() {
        let f = Fixture::new();
        let (o, _, _) = legacy_fixture(&f, "quoted \"; $`path`\n");
        let p = o.bin_dir.join("marmishd");
        fs::write(&p, b"foreign").unwrap();
        println!("EXPECTED_PATH={}", serde_json::to_string(&p).unwrap());
        assert!(matches!(run(o), Err(Error::Invalid("unrelated_launcher"))));
        return;
    }
    let output=std::process::Command::new(std::env::current_exe().unwrap()).args(["--exact","native_install::tests::legacy_foreign_launcher_diagnostic_quotes_exact_path_in_separate_process","--nocapture"]).env(CHILD,"1").output().unwrap();
    assert!(output.status.success(), "{output:?}");
    let stdout = String::from_utf8(output.stdout).unwrap();
    let expected = legacy_diagnostic_path(&stdout)
        .unwrap_or_else(|| panic!("missing or malformed diagnostic path marker: {stdout:?}"));
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr.contains(&format!("conflicting launcher {expected};")),
        "{stderr}"
    );
    assert!(stderr.contains("different --bin-dir"));
    assert!(expected.contains("\\n"));
    assert_eq!(
        stderr.lines().count(),
        1,
        "path control characters must stay escaped"
    );
}
#[test]
fn legacy_occupied_backup_and_partial_restore_preserve_owned_evidence() {
    let f = Fixture::new();
    let (o, _, originals) = legacy_fixture(&f, "old");
    let bin = o.bin_dir.clone();
    FAULT.with(|v| *v.borrow_mut() = Some("transaction"));
    assert!(run(o).is_err());
    FAULT.with(|v| *v.borrow_mut() = None);
    let tx: Transaction = json(&f.prefix().join(TRANSACTION), 0o600, 8192).unwrap();
    let r = read_receipt(&f.prefix()).unwrap();
    let v = tx.legacy.as_ref().unwrap();
    let backup = v.backup(&r, &tx, "marmish");
    fs::write(&backup, b"foreign backup").unwrap();
    assert!(legacy::validate_destinations(&r, &tx).is_err());
    assert!(legacy_status(&f, &bin).is_err());
    assert_eq!(fs::read(&backup).unwrap(), b"foreign backup");
    assert_eq!(fs::read(bin.join("marmish")).unwrap(), originals[0]);
    fs::remove_file(&backup).unwrap();
    // Model the exact durable filesystem state after first restore completed and
    // second restore is still pending, then drive the real production recovery.
    let second = v.backup(&r, &tx, "marmishd");
    fs::rename(bin.join("marmishd"), &second).unwrap();
    assert!(!legacy_status(&f, &bin).unwrap().active);
    for (i, n) in LAUNCHERS.iter().enumerate() {
        assert_eq!(fs::read(bin.join(n)).unwrap(), originals[i]);
    }
}
