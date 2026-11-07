//! Closed interruption evidence, linked with the canonical copy driver only.
//! Public records are observations, never execution or cleanup authority.
use super::{copy_checkpoint::Proof, streaming_cleanup::StreamStatus};
use crate::{Error, Result};
use serde::{Deserialize, Serialize};
pub(super) const PAYLOAD_BYTES: u64 =
    marmish_stream::copy_protocol::CHECKPOINT_INTERVAL + 32 * marmish_stream::MAX_BLOCK as u64;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Binding {
    pub stream_sha256: String,
    pub session_sha256: String,
    pub open_digest: String,
    pub group_sha256: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Evidence {
    pub direction: String,
    pub wall_ns: u64,
    pub resume_wall_ns: u64,
    pub partial: Proof,
    /// Stable durable prefix after confirmed old-helper termination, before resume.
    pub resume_prefix: Proof,
    pub old: Binding,
    pub old_charged_while_client_detached: bool,
    /// A PUT receiver retains its lock after client death; a GET receiver does not.
    pub put_busy: Option<Binding>,
    pub old_cancelled: bool,
    pub after_cancel: StreamStatus,
    pub resumed: Binding,
    pub committed: Proof,
    pub final_sha256: String,
    pub final_size: u64,
    pub final_mode: u32,
    pub final_mtime: i64,
}
fn bad() -> Error {
    Error::Invalid("installed_copy_interrupt")
}
fn digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
impl Binding {
    fn validate(&self) -> Result<()> {
        if [
            &self.stream_sha256,
            &self.session_sha256,
            &self.open_digest,
            &self.group_sha256,
        ]
        .into_iter()
        .all(|x| digest(x))
        {
            Ok(())
        } else {
            Err(bad())
        }
    }
}
pub(super) fn expected_payload_sha256() -> String {
    use sha2::{Digest, Sha256};
    let pattern: Vec<u8> = (0..256)
        .map(|n| (n as u8).wrapping_mul(73).wrapping_add(19))
        .collect();
    let mut hash = Sha256::new();
    let mut remaining = PAYLOAD_BYTES;
    while remaining > 0 {
        let n = remaining.min(pattern.len() as u64) as usize;
        hash.update(&pattern[..n]);
        remaining -= n as u64;
    }
    hex::encode(hash.finalize())
}
impl Evidence {
    /// Independently rechecked by rollup; the driver additionally checks private
    /// raw principals, handles, checkpoint inode bindings and actual file bytes.
    pub(super) fn validate(&self) -> Result<()> {
        self.old.validate()?;
        self.resumed.validate()?;
        self.after_cancel.validate_idle()?;
        if (self.direction != "put" && self.direction != "get")
            || self.wall_ns == 0
            || self.wall_ns > 1_140_000_000_000
            || self.resume_wall_ns == 0
            || self.resume_wall_ns > self.wall_ns
        {
            return Err(bad());
        }
        match (&self.put_busy, self.direction.as_str()) {
            (Some(busy), "put") => {
                busy.validate()?;
                if busy.stream_sha256 == self.old.stream_sha256
                    || busy.stream_sha256 == self.resumed.stream_sha256
                    || busy.session_sha256 != self.old.session_sha256
                    || busy.group_sha256 != self.old.group_sha256
                {
                    return Err(bad());
                }
            }
            (None, "get") => {}
            _ => return Err(bad()),
        }
        let p = &self.partial;
        let c = &self.committed;
        let r = &self.resume_prefix;
        if r.phase != "receiving"
            || r.durable_offset < p.durable_offset
            || (r.durable_offset == p.durable_offset && r.prefix_sha256 != p.prefix_sha256)
            || r.durable_offset >= r.size
            || r.size != p.size
            || r.transfer_sha256 != p.transfer_sha256
            || r.manifest_sha256 != p.manifest_sha256
            || r.sha256 != p.sha256
            || r.stream_sha256 != p.stream_sha256
            || !digest(&r.prefix_sha256)
        {
            return Err(bad());
        }
        if !self.old_charged_while_client_detached
            || !self.old_cancelled
            || p.phase != "receiving"
            || c.phase != "committed"
            || p.size != PAYLOAD_BYTES
            || p.durable_offset == 0
            || p.durable_offset >= p.size
            || c.durable_offset != c.size
            || c.size != p.size
            || p.transfer_sha256 != c.transfer_sha256
            || p.manifest_sha256 != c.manifest_sha256
            || p.sha256 != c.sha256
            || c.prefix_sha256 != c.sha256
            || p.stream_sha256 != self.old.stream_sha256
            || c.stream_sha256 != self.resumed.stream_sha256
            || self.old.stream_sha256 == self.resumed.stream_sha256
            || self.old.session_sha256 != self.resumed.session_sha256
            || self.old.group_sha256 != self.resumed.group_sha256
            || p.sha256 != expected_payload_sha256()
            || self.final_sha256 != c.sha256
            || self.final_size != c.size
            || self.final_mode != 0o600
            || self.final_mtime != 1_700_000_000
            || [
                &p.transfer_sha256,
                &p.manifest_sha256,
                &p.prefix_sha256,
                &p.sha256,
                &c.transfer_sha256,
                &c.manifest_sha256,
                &c.prefix_sha256,
                &c.sha256,
                &self.final_sha256,
            ]
            .into_iter()
            .any(|x| !digest(x))
        {
            return Err(bad());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn binding(n: u8) -> Binding {
        Binding {
            stream_sha256: hex::encode([n; 32]),
            session_sha256: hex::encode([4; 32]),
            open_digest: hex::encode([n + 1; 32]),
            group_sha256: hex::encode([9; 32]),
        }
    }
    pub(super) fn evidence(direction: &str) -> Evidence {
        let old = binding(1);
        let resumed = binding(2);
        let partial = Proof {
            transfer_sha256: hex::encode([5; 32]),
            stream_sha256: old.stream_sha256.clone(),
            manifest_sha256: hex::encode([6; 32]),
            phase: "receiving".into(),
            durable_offset: 1_048_576,
            prefix_sha256: hex::encode([7; 32]),
            size: PAYLOAD_BYTES,
            sha256: expected_payload_sha256(),
        };
        let mut committed = partial.clone();
        committed.stream_sha256 = resumed.stream_sha256.clone();
        committed.phase = "committed".into();
        committed.durable_offset = committed.size;
        committed.prefix_sha256 = committed.sha256.clone();
        Evidence {
            direction: direction.into(),
            wall_ns: 2_000_000,
            resume_wall_ns: 1_000_000,
            resume_prefix: partial.clone(),
            partial,
            old,
            old_charged_while_client_detached: true,
            put_busy: (direction == "put").then(|| binding(3)),
            old_cancelled: true,
            after_cancel: StreamStatus {
                active: 0,
                cancelling: 0,
                uncertain: 0,
                owned_runners: 0,
                unfinished_runners: 0,
                observation_pending: false,
                cleanup_ready: true,
                stop_complete: true,
            },
            resumed,
            final_sha256: committed.sha256.clone(),
            final_size: committed.size,
            final_mode: 0o600,
            final_mtime: 1_700_000_000,
            committed,
        }
    }
    #[test]
    fn direction_specific_detach_lock_and_physical_cleanup_are_required() {
        for direction in ["put", "get"] {
            let e = evidence(direction);
            e.validate().unwrap();
            let mut bad = e.clone();
            bad.old_charged_while_client_detached = false;
            assert!(bad.validate().is_err());
            let mut bad = e.clone();
            bad.after_cancel.unfinished_runners = 1;
            assert!(bad.validate().is_err());
            let mut bad = e.clone();
            bad.put_busy = if direction == "put" {
                None
            } else {
                Some(binding(3))
            };
            assert!(bad.validate().is_err());
        }
    }
    #[test]
    fn interruption_evidence_requires_original_budget_room_and_known_payload() {
        let e = evidence("put");
        let mut bad = e.clone();
        bad.wall_ns = 1_140_000_000_001;
        assert!(bad.validate().is_err());
        for room in [true, false] {
            let mut bad = e.clone();
            let busy = bad.put_busy.as_mut().unwrap();
            if room {
                busy.group_sha256 = "aa".repeat(32);
            } else {
                busy.session_sha256 = "aa".repeat(32);
            }
            assert!(bad.validate().is_err());
        }
        let mut bad = e;
        for proof in [&mut bad.partial, &mut bad.resume_prefix, &mut bad.committed] {
            proof.sha256 = "aa".repeat(32);
        }
        bad.committed.prefix_sha256 = "aa".repeat(32);
        bad.final_sha256 = "aa".repeat(32);
        assert!(bad.validate().is_err());
    }
    #[test]
    fn resume_requires_intermediate_prefix_same_transfer_and_fresh_stream() {
        let e = evidence("put");
        let mut bad = e.clone();
        bad.partial.durable_offset = bad.partial.size;
        assert!(bad.validate().is_err());
        let mut bad = e.clone();
        bad.committed.transfer_sha256 = hex::encode([9; 32]);
        assert!(bad.validate().is_err());
        let mut bad = e.clone();
        bad.resumed = bad.old.clone();
        bad.committed.stream_sha256 = bad.old.stream_sha256.clone();
        assert!(bad.validate().is_err());
        let mut bad = e.clone();
        bad.committed.prefix_sha256 = hex::encode([9; 32]);
        assert!(bad.validate().is_err());
        let mut bad = e;
        bad.final_size -= 1;
        assert!(bad.validate().is_err());
    }
}

#[cfg(test)]
mod stable_prefix_tests {
    #[test]
    fn stable_resume_prefix_must_be_monotonic_and_remain_intermediate() {
        // Start from the same validated fixture as the protocol evidence tests.
        let mut value = super::tests::evidence("get");
        value.resume_prefix.durable_offset += 4096;
        value.resume_prefix.prefix_sha256 = hex::encode([9u8; 32]);
        value.validate().unwrap();
        value.resume_prefix.durable_offset = value.partial.durable_offset - 1;
        assert!(value.validate().is_err());
        value.resume_prefix.durable_offset = value.partial.size;
        assert!(value.validate().is_err());
        value.resume_prefix.durable_offset = value.partial.durable_offset;
        assert!(value.validate().is_err()); // equal offsets require equal prefix hashes
        value.resume_prefix.prefix_sha256 = value.partial.prefix_sha256.clone();
        value.validate().unwrap();
        value.resume_prefix.stream_sha256 = value.resumed.stream_sha256.clone();
        assert!(value.validate().is_err());
    }
}
