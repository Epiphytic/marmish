//! Deterministic byte fixture; isolated build tooling, never a product worker.
use super::records;
use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    ffi::OsString,
    io::{Read, Write},
    os::unix::ffi::{OsStrExt, OsStringExt},
    path::{Path, PathBuf},
};
const INPUT: usize = 16 * 1024;
const STREAM: usize = 128 * 1024;
fn bad() -> Error {
    Error::Invalid("installed_fixture")
}
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(super) struct Receipt {
    pub schema: u8,
    pub nonce: String,
    pub vector: u8,
    pub executable_sha256: String,
    pub argv_sha256: String,
    pub stdin_bytes: usize,
    pub stdin_sha256: String,
    pub stdout_bytes: usize,
    pub stdout_sha256: String,
    pub stderr_bytes: usize,
    pub stderr_sha256: String,
    pub exit: u8,
}
pub(super) fn arguments(nonce: &str, vector: u8) -> Result<Vec<OsString>> {
    if !records::hexadecimal(nonce, 64) || vector > 2 {
        return Err(bad());
    }
    Ok(vec![
        nonce.into(),
        vector.to_string().into(),
        OsString::from(""),
        OsString::from("quoted ' \" \\ $() ; * ?"),
        OsString::from("line\n雪"),
        OsString::from_vec(vec![b'e', 0xff, b'd']),
    ])
}
pub(super) fn bytes(nonce: &str, vector: u8) -> Result<(Vec<u8>, Vec<u8>, Vec<u8>)> {
    let _ = arguments(nonce, vector)?;
    let key = hex::decode(nonce).map_err(|_| bad())?;
    let input = (0..INPUT)
        .map(|i| {
            (i as u8)
                .wrapping_add(key[(i / 256) % 32])
                .wrapping_add(vector)
        })
        .collect();
    let stream = |label: u8| {
        (0..STREAM)
            .map(|i| {
                (i as u8)
                    .wrapping_mul(17)
                    .wrapping_add(key[(i / 256) % 32])
                    .wrapping_add(label)
                    .wrapping_add(vector)
            })
            .collect()
    };
    Ok((input, stream(23), stream(149)))
}
pub(super) fn receipt_path(executable: &Path, nonce: &str) -> Result<PathBuf> {
    if !records::hexadecimal(nonce, 64) || !executable.is_absolute() {
        return Err(bad());
    }
    Ok(executable
        .parent()
        .ok_or_else(bad)?
        .join("receipts")
        .join(format!("{nonce}.receipt")))
}
pub(super) fn argv_digest(executable: &Path, args: &[OsString]) -> Result<String> {
    let mut h = Sha256::new();
    for arg in std::iter::once(executable.as_os_str()).chain(args.iter().map(OsString::as_os_str)) {
        let b = arg.as_bytes();
        let n = u32::try_from(b.len()).map_err(|_| bad())?;
        h.update(n.to_be_bytes());
        h.update(b);
    }
    Ok(hex::encode(h.finalize()))
}
pub(super) fn expected_receipt(
    executable: &Path,
    nonce: &str,
    vector: u8,
    executable_sha256: &str,
) -> Result<Receipt> {
    let args = arguments(nonce, vector)?;
    let (input, stdout, stderr) = bytes(nonce, vector)?;
    Ok(Receipt {
        schema: 1,
        nonce: nonce.into(),
        vector,
        executable_sha256: executable_sha256.into(),
        argv_sha256: argv_digest(executable, &args)?,
        stdin_bytes: input.len(),
        stdin_sha256: records::digest(&input),
        stdout_bytes: stdout.len(),
        stdout_sha256: records::digest(&stdout),
        stderr_bytes: stderr.len(),
        stderr_sha256: records::digest(&stderr),
        exit: 7,
    })
}
fn execute() -> Result<u8> {
    let supplied: Vec<_> = std::env::args_os().collect();
    if supplied.len() != 7 {
        return Err(bad());
    }
    let executable = std::env::current_exe()?.canonicalize()?;
    if supplied[0].as_bytes() != executable.as_os_str().as_bytes() {
        return Err(bad());
    }
    let nonce = supplied[1].to_str().ok_or_else(bad)?;
    let vector = match supplied[2].to_str() {
        Some("0") => 0,
        Some("1") => 1,
        Some("2") => 2,
        _ => return Err(bad()),
    };
    let args = arguments(nonce, vector)?;
    if supplied[1..] != args {
        return Err(bad());
    }
    let (input, stdout, stderr) = bytes(nonce, vector)?;
    let mut actual = Vec::new();
    std::io::stdin()
        .take((INPUT + 1) as u64)
        .read_to_end(&mut actual)?;
    if actual != input {
        return Err(bad());
    }
    {
        let mut out = std::io::stdout().lock();
        out.write_all(&stdout)?;
        out.flush()?;
    }
    {
        let mut out = std::io::stderr().lock();
        out.write_all(&stderr)?;
        out.flush()?;
    }
    let executable_bytes = crate::native_manifest::read_bounded(&executable, 256 * 1024 * 1024)?;
    let receipt = expected_receipt(
        &executable,
        nonce,
        vector,
        &records::digest(&executable_bytes),
    )?;
    records::write(&receipt_path(&executable, nonce)?, &receipt)?;
    Ok(7)
}
pub(crate) fn entry() -> u8 {
    if std::env::args_os().count() == 2
        && std::env::args_os()
            .nth(1)
            .is_some_and(|a| a == "--release-info")
    {
        return super::release_info();
    }
    match execute() {
        Ok(exit) => exit,
        Err(_) => {
            eprintln!("marmish-installed-fixture: invalid_or_failed");
            125
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fixed_vectors_have_every_byte_and_distinct_streams() {
        let n = "12".repeat(32);
        for v in 0..3 {
            let (i, o, e) = bytes(&n, v).unwrap();
            assert_eq!((i.len(), o.len(), e.len()), (16384, 131072, 131072));
            assert_eq!(
                i.iter()
                    .copied()
                    .collect::<std::collections::BTreeSet<_>>()
                    .len(),
                256
            );
            assert_ne!(o, e);
            assert!(
                arguments(&n, v)
                    .unwrap()
                    .last()
                    .unwrap()
                    .as_bytes()
                    .contains(&255)
            );
        }
    }
    #[test]
    fn nonce_cannot_control_receipt_path() {
        assert!(receipt_path(Path::new("/tmp/main"), "../../x").is_err());
        assert_eq!(
            receipt_path(Path::new("/tmp/f/main"), &"1".repeat(64)).unwrap(),
            PathBuf::from(format!("/tmp/f/receipts/{}.receipt", "1".repeat(64)))
        );
    }
}
