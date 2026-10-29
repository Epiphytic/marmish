//! Native master bindings and the bounded private worker handoff.
use crate::{cipher::StorageKeys, launcher::StateRoot, EngineError, JobId, Result};
use serde::{Deserialize, Serialize};
use std::os::fd::AsRawFd;
use zeroize::Zeroizing;

/// Supplied key material; never serializable or printable.
pub struct ProvidedMaster(pub(crate) Zeroizing<[u8; 32]>);
impl ProvidedMaster {
    pub fn new(bytes: [u8; 32]) -> Self {
        Self(Zeroizing::new(bytes))
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativeBinding {
    pub(crate) root_id: [u8; 32],
    pub(crate) key_id: [u8; 32],
}
impl NativeBinding {
    pub fn new(root_id: [u8; 32], key_id: [u8; 32]) -> Self {
        Self { root_id, key_id }
    }
}
#[derive(Clone)]
pub(crate) enum StorageMode {
    Files,
    Native(NativeBinding),
}
#[derive(Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct Reference {
    version: u8,
    purpose: String,
    id: String,
}
#[derive(Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct Marker {
    version: u8,
    backend: String,
    purpose: String,
    reference: Reference,
    root_id: String,
}
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
impl NativeBinding {
    pub(crate) fn marker(&self) -> Marker {
        Marker {
            version: 1,
            backend: "native".into(),
            purpose: "sqlcipher".into(),
            reference: Reference {
                version: 1,
                purpose: "sqlcipher".into(),
                id: hex(&self.key_id),
            },
            root_id: hex(&self.root_id),
        }
    }
}
const MAGIC: &[u8; 8] = b"MRMKEY02";
pub(crate) fn frame(
    root: &StateRoot,
    job: JobId,
    mode: &StorageMode,
    keys: &StorageKeys,
) -> Result<Zeroizing<[u8; 144]>> {
    let mut frame = Zeroizing::new([0u8; 144]);
    frame[..8].copy_from_slice(MAGIC);
    frame[16..32].copy_from_slice(&job.0);
    let (dev, ino) = root.identity()?;
    frame[32..40].copy_from_slice(&dev.to_le_bytes());
    frame[40..48].copy_from_slice(&ino.to_le_bytes());
    if let StorageMode::Native(binding) = mode {
        frame[8] = 1;
        frame[48..80].copy_from_slice(&binding.root_id);
        frame[80..112].copy_from_slice(&binding.key_id);
    }
    frame[112..].copy_from_slice(keys.0.as_ref());
    Ok(frame)
}
pub(crate) fn receive(root: &StateRoot, job: JobId) -> Result<StorageKeys> {
    let deadline = crate::platform::Deadline::after_millis(5000)?;
    let mut frame = Zeroizing::new([0u8; 145]);
    let mut offset = 0;
    loop {
        if deadline.expired()? {
            return Err(EngineError::WorkerUnavailable);
        }
        let mut pfd = libc::pollfd {
            fd: 0,
            events: libc::POLLIN,
            revents: 0,
        };
        let result = unsafe { libc::poll(&mut pfd, 1, 25) };
        if result < 0 {
            return Err(EngineError::WorkerUnavailable);
        }
        if result == 0 {
            continue;
        }
        // Read directly: the process-global Stdin buffer must never retain a key.
        let count =
            unsafe { libc::read(0, frame[offset..].as_mut_ptr().cast(), frame.len() - offset) };
        if count < 0 {
            return Err(EngineError::WorkerUnavailable);
        }
        let count = count as usize;
        if count == 0 {
            break;
        }
        offset += count;
        if offset > 144 {
            return Err(EngineError::InvalidRequest);
        }
    }
    let null = std::fs::File::open("/dev/null").map_err(|_| EngineError::WorkerUnavailable)?;
    if unsafe { libc::dup2(null.as_raw_fd(), 0) } < 0 {
        return Err(EngineError::WorkerUnavailable);
    }
    if offset != 144 || &frame[..8] != MAGIC || frame[9..16] != [0; 7] || frame[16..32] != job.0 {
        return Err(EngineError::InvalidRequest);
    }
    let (dev, ino) = root.identity()?;
    if frame[32..40] != dev.to_le_bytes() || frame[40..48] != ino.to_le_bytes() {
        return Err(EngineError::UnsafePath);
    }
    match frame[8] {
        0 if frame[48..112] == [0; 64] => root.validate_files()?,
        1 => {
            let binding = NativeBinding::new(
                frame[48..80].try_into().unwrap(),
                frame[80..112].try_into().unwrap(),
            );
            root.validate_native(&binding, false)?;
        }
        _ => return Err(EngineError::InvalidRequest),
    }
    let mut master = Zeroizing::new([0; 32]);
    master.copy_from_slice(&frame[112..144]);
    if frame[8] == 0 && root.load_storage_keys()?.0.as_ref() != master.as_ref() {
        return Err(EngineError::StorageFailure);
    }
    Ok(StorageKeys(master))
}
