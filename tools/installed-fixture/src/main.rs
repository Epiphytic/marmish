//! Fixed-role CI executable. No runtime path or action selection exists.
use sha2::{Digest, Sha256};
use std::fs::OpenOptions;
use std::io::{Read, Write};
use std::os::unix::{ffi::OsStrExt, fs::OpenOptionsExt};

const EDGE: &[&[u8]] = &[
    b"",
    b"two words",
    b"'single'",
    b"\"double\"",
    b"$HOME",
    b"$(id)",
    b"`id`",
    b"line\nbreak",
    b"back\\slash",
    b"**/*",
    "雪☃".as_bytes(),
    b"\xff",
];

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn digest(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

fn decode(value: &str, size: usize) -> Option<Vec<u8>> {
    if value.len() != size * 2
        || !value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return None;
    }
    (0..size)
        .map(|i| u8::from_str_radix(&value[i * 2..i * 2 + 2], 16).ok())
        .collect()
}

fn root(path: &str, suffix: &str) -> Option<String> {
    let base = path.strip_suffix(suffix)?;
    let token = base
        .strip_prefix("/tmp/mci-")
        .or_else(|| base.strip_prefix("/private/tmp/mci-"))?;
    if !(6..=16).contains(&token.len())
        || !token
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    {
        return None;
    }
    Some(base.into())
}

fn arm_timeout() -> Option<()> {
    unsafe {
        let mut signals: libc::sigset_t = std::mem::zeroed();
        libc::sigemptyset(&mut signals);
        libc::sigaddset(&mut signals, libc::SIGALRM);
        if libc::sigprocmask(libc::SIG_UNBLOCK, &signals, std::ptr::null_mut()) != 0
            || libc::signal(libc::SIGALRM, libc::SIG_DFL) == libc::SIG_ERR
        {
            return None;
        }
        libc::alarm(120);
    }
    Some(())
}

fn run() -> Option<i32> {
    let role = option_env!("MARMISH_FIXTURE_ROLE")?;
    if !["linux-x64", "linux-arm64", "macos-arm64"].contains(&role) {
        return None;
    }
    let context = option_env!("MARMISH_FIXTURE_CONTEXT")?;
    let context_bytes = decode(context, 32)?;
    let challenge = option_env!("MARMISH_FIXTURE_CHALLENGE")?;
    decode(challenge, 32)?;
    let invocation = option_env!("MARMISH_FIXTURE_INVOCATION")?;
    decode(invocation, 16)?;
    let executable = option_env!("MARMISH_FIXTURE_EXECUTABLE")?;
    let receipt = option_env!("MARMISH_FIXTURE_RECEIPT")?;
    if root(executable, &format!("/f/{role}/main"))? != root(receipt, &format!("/r/{role}.json"))? {
        return None;
    }
    // Configuration validation above performs no filesystem operations.
    arm_timeout()?;
    let mut record = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(receipt)
        .ok()?;
    let args: Vec<Vec<u8>> = std::env::args_os().map(|v| v.as_bytes().to_vec()).collect();
    if args.iter().map(Vec::len).sum::<usize>() > 16384 {
        return None;
    }
    let mut expected = vec![
        executable.as_bytes(),
        challenge.as_bytes(),
        invocation.as_bytes(),
    ];
    expected.extend_from_slice(EDGE);
    if args.iter().map(Vec::as_slice).collect::<Vec<_>>() != expected {
        return None;
    }
    let mut input = Vec::new();
    std::io::stdin().take(8193).read_to_end(&mut input).ok()?;
    let mut wanted = b"marmish-stdin-v1\0".to_vec();
    wanted.extend_from_slice(&context_bytes);
    let prefix = wanted.len();
    wanted.extend((0..8192 - prefix).map(|n| (n % 256) as u8));
    if input != wanted {
        return None;
    }
    let mut encoded = Vec::new();
    for arg in &args {
        encoded.extend_from_slice(&(arg.len() as u32).to_be_bytes());
        encoded.extend_from_slice(arg);
    }
    let mut output = b"MARMISH-INTEROP\0\x01".to_vec();
    output.extend_from_slice(&encoded);
    for counter in 0u32..2048 {
        let mut hash = Sha256::new();
        hash.update(b"marmish-output-v1\0");
        hash.update(&context_bytes);
        hash.update(counter.to_be_bytes());
        output.extend_from_slice(&hash.finalize());
    }
    let error: Vec<u8> = input.iter().rev().copied().collect();
    std::io::stdout().write_all(&output).ok()?;
    std::io::stdout().flush().ok()?;
    std::io::stderr().write_all(&error).ok()?;
    std::io::stderr().flush().ok()?;
    let value = serde_json::json!({"format":1,"kind":"fixture-receipt","role":role,
        "context":context,"challenge":challenge,"invocation":invocation,
        "argv_sha256":digest(&encoded),"stdin_sha256":digest(&input),
        "stdout_bytes":output.len(),"stdout_sha256":digest(&output),
        "stderr_bytes":error.len(),"stderr_sha256":digest(&error),"outcome":7});
    let mut raw = serde_json::to_vec(&value).ok()?;
    raw.push(b'\n');
    record.write_all(&raw).ok()?;
    record.flush().ok()?;
    record.sync_all().ok()?;
    Some(7)
}

fn main() {
    std::process::exit(run().unwrap_or(125));
}

#[cfg(test)]
mod tests {
    #[test]
    fn timeout_is_armed_for_120_seconds() {
        assert_eq!(super::arm_timeout(), Some(()));
        let remaining = unsafe { libc::alarm(0) };
        assert!((119..=120).contains(&remaining));
    }
}
