use std::{fs, process::Command};
fn main() {
    const CANDIDATE_CFG: &str = "CARGO_CFG_MARMISH_PEER_RECEIVER_CANDIDATE";
    println!("cargo:rustc-check-cfg=cfg(marmish_peer_receiver_candidate)");
    println!("cargo:rerun-if-env-changed=MARMISH_BUILD_COMMIT");
    println!("cargo:rerun-if-env-changed={CANDIDATE_CFG}");
    let candidate = std::env::var_os(CANDIDATE_CFG).is_some();
    let target = std::env::var("TARGET").expect("Cargo target");
    let explicit_commit = std::env::var("MARMISH_BUILD_COMMIT").ok();
    if candidate {
        assert_eq!(
            target, "x86_64-unknown-linux-gnu",
            "unsupported Peer candidate target"
        );
        assert!(
            std::env::var_os("CARGO_FEATURE_INSTALLED_CHECKS").is_none(),
            "Peer candidate cannot include installed-checks"
        );
        assert!(
            explicit_commit
                .as_ref()
                .is_some_and(|commit| commit.bytes().any(|byte| byte != b'0')),
            "Peer candidate requires an explicit nonzero build commit"
        );
    }
    // Track the actual worktree HEAD and symbolic ref, including linked worktrees.
    let mut refs = vec!["HEAD".to_owned()];
    if let Ok(output) = Command::new("git")
        .args(["symbolic-ref", "-q", "HEAD"])
        .output()
        && output.status.success()
        && let Ok(reference) = String::from_utf8(output.stdout)
    {
        refs.push(reference.trim().to_owned());
    }
    for reference in refs {
        if let Ok(output) = Command::new("git")
            .args(["rev-parse", "--git-path", &reference])
            .output()
            && output.status.success()
            && let Ok(path) = String::from_utf8(output.stdout)
        {
            println!("cargo:rerun-if-changed={}", path.trim());
        }
    }
    let commit = explicit_commit.unwrap_or_else(|| {
        Command::new("git")
            .args(["rev-parse", "HEAD"])
            .output()
            .ok()
            .filter(|o| o.status.success())
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .map(|s| s.trim().to_owned())
            .unwrap_or_else(|| "0".repeat(40))
    });
    assert!(
        commit.len() == 40
            && commit
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
        "invalid build commit"
    );
    println!("cargo:rustc-env=MARMISH_BUILD_TARGET={target}");
    println!("cargo:rustc-env=MARMISH_BUILD_COMMIT={commit}");
    println!(
        "cargo:rustc-env=MARMISH_BUILD_PEER_CANDIDATE={}",
        u8::from(candidate)
    );
    const LOCK: &str = "../../Cargo.lock";
    println!("cargo:rerun-if-changed={LOCK}");
    let lock = fs::read_to_string(LOCK).ok();
    let mdk = lock
        .as_deref()
        .and_then(locked_mdk_revision)
        .unwrap_or_default();
    println!("cargo:rustc-env=MARMISH_MDK_REVISION={mdk}");
}

// Cargo generates one source line per locked package in this format.
fn locked_mdk_revision(lock: &str) -> Option<&str> {
    const REPOSITORY: &str = "git+https://github.com/marmot-protocol/mdk";
    const MASTER: &str = "?branch=master#";
    const DIRECT: [&str; 6] = [
        "cgka-engine",
        "cgka-traits",
        "marmot-session",
        "storage-sqlite",
        "transport-nostr-adapter",
        "transport-nostr-peeler",
    ];
    let mut direct = std::collections::BTreeSet::new();
    let mut revision = None;
    for package in lock
        .split(
            "
[[package]]
",
        )
        .skip(1)
    {
        let name = lock_string(package, "name = \"");
        let source = lock_string(package, "source = \"");
        let root = name.filter(|name| DIRECT.contains(name));
        let mdk = match source.and_then(|source| source.strip_prefix(REPOSITORY)) {
            Some(source) => Some(source.strip_prefix(MASTER)?),
            None => None,
        };
        if let Some(root) = root
            && (!direct.insert(root) || mdk.is_none())
        {
            return None;
        }
        if let Some(mdk) = mdk {
            if mdk.len() != 40
                || !mdk
                    .bytes()
                    .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
                || revision.is_some_and(|revision| revision != mdk)
            {
                return None;
            }
            revision = Some(mdk);
        }
    }
    if direct.len() == DIRECT.len() {
        revision
    } else {
        None
    }
}

fn lock_string<'a>(package: &'a str, prefix: &str) -> Option<&'a str> {
    package
        .lines()
        .find_map(|line| line.strip_prefix(prefix)?.strip_suffix('"'))
}
