//! Leased staged-Consent fixture. No connector or production route.

use crate::command_config::Role;
use std::{
    fs::{self, Metadata, OpenOptions},
    io::{self, Read},
    os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    path::Path,
};
use storage_sqlite::SqlCipherKey;
use tempfile::TempDir;

use super::*;
use consent_checkpoint::provisioning::{
    Binding, Reservation,
    stage_journal::{Artifact, FileWitness},
};

#[path = "peer_prepared_consent_owner_tests.rs"]
mod tests;

struct SnapshotArea(TempDir);

impl SnapshotArea {
    fn new() -> Result<Self> {
        tempfile::Builder::new()
            .permissions(fs::Permissions::from_mode(0o700))
            .tempdir()
            .map(Self)
            .map_err(|_| Error::Path)
    }

    fn read(
        &self,
        state: &InvitationState<'_>,
        binding: Binding,
        witness: FileWitness,
        sample: Duration,
        before_open: impl FnOnce(&Path) -> Result<()>,
    ) -> Result<(NativePackage, [u8; 32])> {
        let original = state.paths.reopen_provisional(binding, witness)?;
        let digest = original.digest()?;
        let path = self.0.path().join("image.db");
        snapshot_sidecars(&path)?;
        let mut copy = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(&path)
            .map_err(|_| Error::Path)?;
        let result = (|| {
            let inode = snapshot_inode(&copy.metadata().map_err(|_| Error::Path)?)?;
            let copied = io::copy(
                &mut original.read_file()?.take(paths::MAX_STAGE_LEN + 1),
                &mut copy,
            )
            .map_err(|_| Error::Path)?;
            drop(copy);
            if copied == 0
                || copied > paths::MAX_STAGE_LEN
                || read_image(&path)? != (digest, inode)
                || original.digest()? != digest
            {
                return Err(Error::StageIdentityMismatch);
            }
            before_open(&path)?;
            if read_image(&path)? != (digest, inode) {
                return Err(Error::StageIdentityMismatch);
            }
            let native = OpenStage::open_provisional(
                &path,
                &SqlCipherKey::raw(*state.paths.provisional_key(binding)),
                state.signer.clone(),
                &state.origin_url,
                state.paths.provisional_label(binding),
                false,
            )?;
            let package = native.read_provisional(sample)?.event;
            drop(native);
            if read_image(&path)? != (digest, inode) || original.digest()? != digest {
                return Err(Error::StageIdentityMismatch);
            }
            Ok((package, digest))
        })();
        // Leave an unresolved name or sidecar obstructing the next owner turn.
        // TempDir owns cleanup of this fixture on final teardown.
        fs::remove_file(&path).map_err(|_| Error::Path)?;
        snapshot_sidecars(&path)?;
        result
    }
}

fn snapshot_sidecars(path: &Path) -> Result<()> {
    for suffix in ["-journal", "-wal", "-shm"] {
        let mut name = path.as_os_str().to_owned();
        name.push(suffix);
        match fs::symlink_metadata(name) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            _ => return Err(Error::StageIdentityMismatch),
        }
    }
    Ok(())
}

fn snapshot_inode(metadata: &Metadata) -> Result<(u64, u64)> {
    if !metadata.is_file() || metadata.nlink() != 1 || metadata.mode() & 0o7777 != 0o600 {
        return Err(Error::StageIdentityMismatch);
    }
    Ok((metadata.dev(), metadata.ino()))
}

fn read_image(path: &Path) -> Result<([u8; 32], (u64, u64))> {
    use sha2::Digest;
    snapshot_sidecars(path)?;
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)
        .map_err(|_| Error::Path)?;
    let inode = snapshot_inode(&file.metadata().map_err(|_| Error::Path)?)?;
    let mut digest = Sha256::new();
    let sum =
        io::copy(&mut file.take(paths::MAX_STAGE_LEN + 1), &mut digest).map_err(|_| Error::Path)?;
    if sum == 0 || sum > paths::MAX_STAGE_LEN {
        return Err(Error::StageIdentityMismatch);
    }
    Ok((digest.finalize().into(), inode))
}

struct PreparedConsentOwner<'a> {
    state: InvitationState<'a>,
    role: Role,
    snapshot: SnapshotArea,
}

impl<'a> PreparedConsentOwner<'a> {
    fn open(
        lease: &'a ProfileLease,
        material: &IdentityMaterial,
        role: Role,
        policy: Policy,
        route: &Route,
        wall: Wall,
    ) -> Result<Self> {
        if role == Role::Server {
            return Err(Error::Binding);
        }
        let paths = Paths::new(lease, material)?;
        let checked = paths.cursor()?;
        let conn = HardenedPeerConnection::open(checked.path(), *paths.cursor_key())?;
        checked.unchanged()?;
        let signer = material.signer();
        let url = &route.urls[0];
        // SAFETY: reads the Unix effective UID.
        let uid = unsafe { libc::geteuid() };
        let checkpoint = Checkpoint::attach_prepared_consent_fixture(
            conn,
            (*signer).clone(),
            uid,
            &[url],
            policy,
        )?;
        let source = checkpoint.bind_source(url)?;
        checked.unchanged()?;
        Ok(Self {
            state: InvitationState {
                paths,
                signer,
                origin: route.id,
                checkpoint,
                source,
                wall,
                origin_url: url.clone(),
            },
            role,
            snapshot: SnapshotArea::new()?,
        })
    }

    fn sample(&self) -> Result<Duration> {
        if matches!(self.role, Role::Client | Role::Both) {
            self.state.provisional_sample()
        } else {
            Err(Error::Binding)
        }
    }
}

pub(super) struct NativeAttestation {
    binding: Binding,
    witness: FileWitness,
    package: NativePackage,
    image: [u8; 32],
}

impl NativeAttestation {
    pub(super) fn require(
        &self,
        stage: (Binding, FileWitness),
        artifact: Artifact,
        wall: u64,
    ) -> consent_checkpoint::Result<&[u8]> {
        let found = Artifact::new(self.package.id, &self.package.bytes, self.package.expires);
        if stage != (self.binding, self.witness) || artifact != found || wall >= found.expires {
            return Err(consent_checkpoint::Error::Package);
        }
        Ok(&self.package.bytes)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Cut {
    BeforeSign,
    AfterSign,
    BeforeQueueCommit,
    Queued,
}

impl PreparedConsentOwner<'_> {
    fn attest(
        &self,
        (binding, witness): (Binding, FileWitness),
        sample: Duration,
    ) -> Result<NativeAttestation> {
        let (package, image) = self
            .snapshot
            .read(&self.state, binding, witness, sample, |_| Ok(()))?;
        Ok(NativeAttestation {
            binding,
            witness,
            package,
            image,
        })
    }

    fn verify_custody(&self, proof: &NativeAttestation) -> Result<()> {
        let file = self
            .state
            .paths
            .reopen_provisional(proof.binding, proof.witness)?;
        if file.digest()? != proof.image {
            return Err(Error::StageIdentityMismatch);
        }
        Ok(())
    }

    fn queue_prepared_consent(
        &mut self,
        reservation: &Reservation,
        mut after: impl FnMut(Cut) -> Result<()>,
    ) -> Result<consent_checkpoint::wire::Transcript> {
        self.state
            .checkpoint
            .require_reservation_owner(reservation)?;
        let (_, _, _, selector) = reservation.stage_binding().coordinates();
        if let Some(transcript) = self.recover_queued_consent(selector)? {
            return Ok(transcript);
        }
        let sample = self.sample()?;
        let candidate = self.state.checkpoint.inspect_prepared_consent(
            &self.state.source,
            reservation,
            sample.as_secs(),
        )?;
        let proof = self.attest(candidate.stage(), sample)?;
        after(Cut::BeforeSign)?;
        let signed = self.state.checkpoint.sign_prepared_consent(
            &candidate,
            &proof,
            self.sample()?.as_secs(),
        )?;
        after(Cut::AfterSign)?;
        self.verify_custody(&proof)?;
        let wall = self.sample()?.as_secs();
        let queued = self.state.checkpoint.queue_prepared_consent(
            &self.state.source,
            reservation,
            signed,
            wall,
            |_| {
                after(Cut::BeforeQueueCommit)
                    .map_err(|_| consent_checkpoint::Error::Cursor(store::Error::Storage))
            },
        )?;
        after(Cut::Queued)?;
        self.verify_custody(&proof)?;
        Ok(self
            .state
            .checkpoint
            .verify_queued_consent(&queued, &proof, wall)?)
    }

    fn recover_queued_consent(
        &mut self,
        selector: Selector,
    ) -> Result<Option<consent_checkpoint::wire::Transcript>> {
        let sample = self.sample()?;
        let Some(queued) = self.state.checkpoint.recover_queued_consent(
            &self.state.source,
            selector,
            sample.as_secs(),
        )?
        else {
            return Ok(None);
        };
        let proof = self.attest(queued.stage(), sample)?;
        let transcript = self.state.checkpoint.verify_queued_consent(
            &queued,
            &proof,
            self.sample()?.as_secs(),
        )?;
        self.verify_custody(&proof)?;
        Ok(Some(transcript))
    }
}

use consent_checkpoint::provisioning::prepared_consent::Attempt;

#[derive(Clone, Copy)]
enum SinkResponse {
    Positive,
    Negative,
    WrongOrigin,
    WrongCarrier,
    WrongSequence,
}

struct FixtureSink {
    owner: Arc<()>,
    origin: Origin,
    ready: bool,
    response: SinkResponse,
    last: Option<Vec<u8>>,
}

struct Receipt {
    owner: Arc<()>,
    origin: Origin,
    carrier: [u8; 32],
    sequence: u64,
}

impl FixtureSink {
    fn ready_for(&self, origin: Origin) -> bool {
        self.ready && self.origin == origin
    }

    fn publish(&mut self, attempt: &Attempt) -> Result<Option<Receipt>> {
        if !self.ready_for(attempt.origin()) {
            return Err(Error::Binding);
        }
        self.last = Some(attempt.bytes().to_vec());
        let mut receipt = Receipt {
            owner: self.owner.clone(),
            origin: self.origin,
            carrier: attempt.carrier(),
            sequence: attempt.sequence(),
        };
        match self.response {
            SinkResponse::Positive => {}
            SinkResponse::Negative => return Ok(None),
            SinkResponse::WrongOrigin => receipt.origin[0] ^= 1,
            SinkResponse::WrongCarrier => receipt.carrier[0] ^= 1,
            SinkResponse::WrongSequence => receipt.sequence ^= 1,
        }
        Ok(Some(receipt))
    }

    fn owns(&self, receipt: &Receipt, attempt: &Attempt) -> bool {
        Arc::ptr_eq(&self.owner, &receipt.owner)
            && self.ready_for(receipt.origin)
            && receipt.origin == attempt.origin()
            && receipt.carrier == attempt.carrier()
            && receipt.sequence == attempt.sequence()
    }
}

struct Reserved {
    attempt: Attempt,
    native: NativeAttestation,
    sink: Arc<()>,
}

struct Sent {
    reserved: Reserved,
    receipt: Receipt,
}

pub(super) struct MatchingAck(Attempt);

impl MatchingAck {
    pub(super) fn into_attempt(self) -> Attempt {
        self.0
    }
}

impl PreparedConsentOwner<'_> {
    fn reserve_prepared_attempt(
        &mut self,
        selector: Selector,
        sink: &FixtureSink,
    ) -> Result<Option<Reserved>> {
        if !sink.ready_for(self.state.origin) {
            return Ok(None);
        }
        let sample = self.sample()?;
        let Some(queued) = self.state.checkpoint.recover_queued_consent(
            &self.state.source,
            selector,
            sample.as_secs(),
        )?
        else {
            return Ok(None);
        };
        let native = self.attest(queued.stage(), sample)?;
        let wall = self.sample()?.as_secs();
        self.verify_custody(&native)?;
        let attempt = self.state.checkpoint.reserve_prepared_attempt(
            &self.state.source,
            queued,
            &native,
            wall,
        )?;
        self.verify_custody(&native)?;
        Ok(attempt.map(|attempt| Reserved {
            attempt,
            native,
            sink: sink.owner.clone(),
        }))
    }

    fn publish_prepared_attempt(
        &mut self,
        reserved: Reserved,
        sink: &mut FixtureSink,
    ) -> Result<Option<Sent>> {
        if !Arc::ptr_eq(&reserved.sink, &sink.owner) {
            return Err(Error::Binding);
        }
        let wall = self.sample()?.as_secs();
        self.verify_custody(&reserved.native)?;
        self.state
            .checkpoint
            .check_current_attempt(&self.state.source, &reserved.attempt, wall)?;
        let result = sink.publish(&reserved.attempt)?;
        self.verify_custody(&reserved.native)?;
        Ok(result.map(|receipt| Sent { reserved, receipt }))
    }

    fn settle_prepared_ack(&mut self, sent: Sent, sink: &FixtureSink) -> Result<()> {
        let Sent { reserved, receipt } = sent;
        if !Arc::ptr_eq(&reserved.sink, &sink.owner) || !sink.owns(&receipt, &reserved.attempt) {
            return Err(Error::Binding);
        }
        let wall = self.sample()?.as_secs();
        self.verify_custody(&reserved.native)?;
        self.state.checkpoint.settle_prepared_ack(
            &self.state.source,
            MatchingAck(reserved.attempt),
            wall,
        )?;
        self.verify_custody(&reserved.native)?;
        Ok(())
    }
}
