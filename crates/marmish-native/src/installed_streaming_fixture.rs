//! Closed build-only fixture modes for installed streaming qualification.
use super::records;
use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use std::{
    ffi::OsString,
    io::{Read, Write},
    os::unix::ffi::OsStrExt,
    path::Path,
};

pub(super) const STDIN_BYTES: usize = 96 * 1024;
pub(super) const STDOUT_BYTES: usize = 96 * 1024;
pub(super) const STDERR_BYTES: usize = 8 * 1024;
fn bad() -> Error {
    Error::Invalid("installed_stream_fixture")
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Receipt {
    pub schema: u8,
    pub nonce: String,
    pub client: String,
    pub case: String,
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

pub(super) fn arguments(nonce: &str, client: &str) -> Result<Vec<OsString>> {
    if !records::hexadecimal(nonce, 64) || !records::ROLES.contains(&client) {
        return Err(bad());
    }
    Ok(vec![
        "stream-v1".into(),
        nonce.into(),
        client.into(),
        "duplex".into(),
        "".into(),
        "quotes ' \" \\ $() ; * ?\n雪".into(),
    ])
}
pub(super) fn bytes(nonce: &str, label: u8, count: usize) -> Result<Vec<u8>> {
    if !records::hexadecimal(nonce, 64) || count > 4 * 1024 * 1024 {
        return Err(bad());
    }
    let key = hex::decode(nonce).map_err(|_| bad())?;
    Ok((0..count)
        .map(|i| {
            (i as u8)
                .wrapping_mul(17)
                .wrapping_add(key[(i / 256) % 32])
                .wrapping_add(label)
        })
        .collect())
}
pub(super) fn argument_digest(nonce: &str, client: &str) -> Result<String> {
    let args: Vec<Vec<u8>> = arguments(nonce, client)?
        .into_iter()
        .map(|a| a.as_bytes().to_vec())
        .collect();
    Ok(records::digest(&records::canonical(&args)?))
}
pub(super) fn receipt(
    _executable: &Path,
    hash: &str,
    nonce: &str,
    client: &str,
) -> Result<Receipt> {
    if !records::hexadecimal(hash, 64) {
        return Err(bad());
    }
    Ok(Receipt {
        schema: 1,
        nonce: nonce.into(),
        client: client.into(),
        case: "duplex".into(),
        executable_sha256: hash.into(),
        argv_sha256: argument_digest(nonce, client)?,
        stdin_bytes: STDIN_BYTES,
        stdin_sha256: records::digest(&bytes(nonce, 7, STDIN_BYTES)?),
        stdout_bytes: STDOUT_BYTES,
        stdout_sha256: records::digest(&bytes(nonce, 43, STDOUT_BYTES)?),
        stderr_bytes: STDERR_BYTES,
        stderr_sha256: records::digest(&bytes(nonce, 149, STDERR_BYTES)?),
        exit: 7,
    })
}
pub(super) fn receipt_path(executable: &Path, nonce: &str) -> Result<std::path::PathBuf> {
    if !records::hexadecimal(nonce, 64) {
        return Err(bad());
    }
    Ok(executable
        .parent()
        .ok_or_else(bad)?
        .join("receipts")
        .join(format!("stream-{nonce}.json")))
}
pub(super) fn execute(args: &[OsString]) -> Result<u8> {
    if args.len() != 6 {
        return Err(bad());
    }
    let nonce = args[1].to_str().ok_or_else(bad)?;
    let client = args[2].to_str().ok_or_else(bad)?;
    if args != arguments(nonce, client)? {
        return Err(bad());
    }
    let executable = std::env::current_exe()?.canonicalize()?;
    let mut input = Vec::new();
    std::io::stdin()
        .take((STDIN_BYTES + 1) as u64)
        .read_to_end(&mut input)?;
    if input != bytes(nonce, 7, STDIN_BYTES)? {
        return Err(bad());
    }
    {
        let mut out = std::io::stdout().lock();
        out.write_all(&bytes(nonce, 43, STDOUT_BYTES)?)?;
        out.flush()?;
    }
    {
        let mut err = std::io::stderr().lock();
        err.write_all(&bytes(nonce, 149, STDERR_BYTES)?)?;
        err.flush()?;
    }
    let hash = records::digest(&records::read_bytes(&executable, 268435456)?);
    records::write(
        &receipt_path(&executable, nonce)?,
        &receipt(&executable, &hash, nonce, client)?,
    )?;
    Ok(7)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn duplex_vectors_span_all_bytes_and_bound_inputs() {
        let nonce = "ab".repeat(32);
        for (label, count) in [(7, STDIN_BYTES), (43, STDOUT_BYTES), (149, STDERR_BYTES)] {
            let b = bytes(&nonce, label, count).unwrap();
            assert_eq!(b.len(), count);
            assert_eq!(
                b.into_iter()
                    .collect::<std::collections::BTreeSet<_>>()
                    .len(),
                256
            );
        }
        assert!(bytes(&nonce, 7, 4 * 1024 * 1024 + 1).is_err());
        assert!(arguments(&nonce, "../role").is_err());
        assert!(arguments("bad", records::ROLES[0]).is_err());
        assert_ne!(
            bytes(&nonce, 7, 256).unwrap(),
            bytes(&nonce, 43, 256).unwrap()
        );
    }
}
