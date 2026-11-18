//! Two fixed, generation-keyed paths in the held profile.

use std::io::{self, Read};

use sha2::Digest as _;

use super::super::store::consent_checkpoint::provisioning::Binding;
use super::super::store::consent_checkpoint::provisioning::stage_journal::FileWitness;
use super::*;

const MAX_STAGE_LEN: u64 = 16 * 1024 * 1024;
const SIDECARS: &[&str] = &["-journal", "-wal", "-shm"];

pub(in super::super) struct ProvisionalFile {
    checked: CheckedFile,
    witness: FileWitness,
}

impl Paths<'_> {
    pub(in super::super) fn require_unallocated(&self, binding: Binding) -> Result<()> {
        self.provisional_device()?;
        let path = self.provisional_path(binding);
        absence(&path)?;
        clean_sidecars(&path)
    }

    pub(in super::super) fn resume_allocation(&self, binding: Binding) -> Result<ProvisionalFile> {
        self.provisional_device()?;
        let path = self.provisional_path(binding);
        clean_sidecars(&path)?;
        match fs::symlink_metadata(&path) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => create_file(&path)?,
            Ok(_) => {}
            Err(_) => return Err(Error::Path),
        }
        let file = self.provisional_file(binding)?;
        if file.read_file()?.metadata().map_err(|_| Error::Path)?.len() != 0 {
            return Err(Error::StageIdentityMismatch);
        }
        Ok(file)
    }

    pub(in super::super) fn reopen_provisional(
        &self,
        binding: Binding,
        witness: FileWitness,
    ) -> Result<ProvisionalFile> {
        let file = self.provisional_file(binding)?;
        if file.witness != witness {
            return Err(Error::StageIdentityMismatch);
        }
        Ok(file)
    }

    fn provisional_file(&self, binding: Binding) -> Result<ProvisionalFile> {
        let expected_device = self.provisional_device()?;
        let path = self.provisional_path(binding);
        clean_sidecars(&path)?;
        let checked = CheckedFile::read(path, &[]).map_err(|_| Error::StageIdentityMismatch)?;
        if checked.before.0 != expected_device {
            return Err(Error::StageIdentityMismatch);
        }
        let witness = FileWitness {
            device: checked.before.0.to_be_bytes(),
            inode: checked.before.1.to_be_bytes(),
        };
        let file = ProvisionalFile { checked, witness };
        file.ensure_unchanged()?;
        Ok(file)
    }

    pub(in super::super) fn provisional_key(&self, binding: Binding) -> Zeroizing<[u8; 32]> {
        self.key(
            b"marmish/peer/provisional-stage/1",
            &binding_bytes(binding.coordinates()),
        )
    }

    pub(in super::super) fn provisional_label(&self, binding: Binding) -> String {
        let key = self.key(
            b"marmish/peer/provisional-slot/1",
            &binding_bytes(binding.coordinates()),
        );
        hex::encode(&key[..16])
    }

    fn provisional_path(&self, binding: Binding) -> PathBuf {
        let name = if binding.coordinates().0 {
            "provisional-known.db"
        } else {
            "provisional-unknown.db"
        };
        self.directory().join(name)
    }

    fn provisional_device(&self) -> Result<u64> {
        let cursor = self.cursor()?;
        let dir = fs::symlink_metadata(self.directory()).map_err(|_| Error::Path)?;
        if dir.dev() != cursor.before.0 {
            return Err(Error::StageIdentityMismatch);
        }
        Ok(dir.dev())
    }
}

fn binding_bytes(
    (known, generation, origin, selector): (bool, u64, [u8; 32], crate::peer_invitation::Selector),
) -> [u8; 121] {
    let mut bytes = [0; 121];
    bytes[0] = u8::from(known);
    bytes[1..9].copy_from_slice(&generation.to_be_bytes());
    bytes[9..41].copy_from_slice(&origin);
    bytes[41..73].copy_from_slice(&selector.server.to_bytes());
    bytes[73..89].copy_from_slice(&selector.id);
    bytes[89..121].copy_from_slice(&selector.revision);
    bytes
}

impl ProvisionalFile {
    pub(in super::super) fn path(&self) -> &Path {
        self.checked.path()
    }

    pub(in super::super) fn witness(&self) -> FileWitness {
        self.witness
    }

    pub(in super::super) fn ensure_unchanged(&self) -> Result<()> {
        self.read_file().map(|_| ())
    }

    pub(in super::super) fn digest(&self) -> Result<[u8; 32]> {
        let mut file = self.read_file()?.take(MAX_STAGE_LEN + 1);
        let mut digest = Sha256::new();
        let copied = io::copy(&mut file, &mut digest).map_err(|_| Error::Path)?;
        if copied == 0 || copied > MAX_STAGE_LEN {
            return Err(Error::StageIdentityMismatch);
        }
        self.ensure_unchanged()?;
        Ok(digest.finalize().into())
    }

    fn read_file(&self) -> Result<File> {
        clean_sidecars(&self.checked.path)?;
        self.checked
            .unchanged()
            .map_err(|_| Error::StageIdentityMismatch)?;
        let file = open_private(&self.checked.path, false).map_err(|_| Error::Path)?;
        let metadata = file.metadata().map_err(|_| Error::Path)?;
        if (metadata.dev(), metadata.ino()) != self.checked.before || metadata.len() > MAX_STAGE_LEN
        {
            return Err(Error::StageIdentityMismatch);
        }
        Ok(file)
    }
}

fn absence(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        _ => Err(Error::StageIdentityMismatch),
    }
}

fn clean_sidecars(path: &Path) -> Result<()> {
    for suffix in SIDECARS {
        let mut name = path.as_os_str().to_owned();
        name.push(suffix);
        absence(Path::new(&name))?;
    }
    Ok(())
}

#[test]
fn peer_stage_key_and_slot_known_answers() {
    use crate::peer_invitation::Selector;
    use nostr::key::PublicKey;
    let keys = Hkdf::new(Some(&[7; 32]), &[9; 32]);
    let derive =
        |domain: &[u8], coordinates: &[u8]| expand_peer_key(&keys, domain, b"fixture", coordinates);
    let selector = Selector {
        server: PublicKey::from_byte_array([4; 32]),
        id: [5; 16],
        revision: [6; 32],
    };
    let fields = (true, 0x0102_0304_0506_0708, [3; 32], selector);
    let encoded = binding_bytes(fields);
    let domain = b"marmish/peer/provisional-stage/1";
    let key = derive(domain, &encoded);
    assert_eq!(
        hex::encode(&key[..]),
        "02b6e2d1389c2932a38b66d44205a1a0a4b5fb46c0eec7e538deccc1382895d3",
    );
    let slot = derive(b"marmish/peer/provisional-slot/1", &encoded);
    assert_eq!(hex::encode(&slot[..16]), "342d75274c73d0545dcc82d7ceededce");
    assert_ne!(
        key,
        derive(
            domain,
            &binding_bytes((fields.0, fields.1 + 1, fields.2, fields.3))
        )
    );
    assert_ne!(key, derive(b"marmish/peer/cursor/2", &[]));
}
