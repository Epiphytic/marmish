use std::{fs, process::Command};
fn main() {
    println!("cargo:rerun-if-env-changed=MARMISH_BUILD_COMMIT");
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
    let target = std::env::var("TARGET").expect("Cargo target");
    let commit = std::env::var("MARMISH_BUILD_COMMIT")
        .ok()
        .unwrap_or_else(|| {
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
