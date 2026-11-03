use std::process::Command;
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
}
