//! Independent native command-engine key domain. No file-key fallback.
use crate::command_wire::Account;
use crate::{Error, Result};
use hkdf::Hkdf;
use marmish_engine::NativeBinding;
use sha2::{Digest as _, Sha256};
use zeroize::Zeroizing;

pub(crate) const DERIVATION_VERSION: u32 = 1;

pub(crate) struct EngineMaterial {
    pub master: Zeroizing<[u8; 32]>,
    pub binding: NativeBinding,
    pub root_id: [u8; 32],
    pub key_id: [u8; 32],
}

/// The profile ID is fixed by the leased profile manifest. Account and profile
/// bind the public references; only the SQLCipher root supplies secret entropy.
pub(crate) fn derive(
    profile_id: &str,
    account: Account,
    sqlcipher_root: &[u8; 32],
) -> Result<EngineMaterial> {
    if !crate::foundation::valid_hex(profile_id, 32) {
        return Err(Error::Identity("command_profile_binding"));
    }
    let mut master = Zeroizing::new([0u8; 32]);
    Hkdf::<Sha256>::new(None, sqlcipher_root)
        .expand(b"marmish/v2/command-engine/1", master.as_mut())
        .map_err(|_| Error::Identity("command_key_derivation"))?;
    let reference = |domain: &[u8]| -> [u8; 32] {
        let mut digest = Sha256::new();
        digest.update(domain);
        digest.update(profile_id.as_bytes());
        digest.update(account);
        digest.finalize().into()
    };
    let root_id = reference(b"marmish/v2/command-engine/root/1\0");
    let key_id = reference(b"marmish/v2/command-engine/key/1\0");
    Ok(EngineMaterial {
        master,
        binding: NativeBinding::new(root_id, key_id),
        root_id,
        key_id,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stable_engine_domain_is_independent_of_other_profile_keys() {
        let profile = "01".repeat(16);
        let first = derive(&profile, [2; 32], &[3; 32]).unwrap();
        let reopened = derive(&profile, [2; 32], &[3; 32]).unwrap();
        assert_eq!(*first.master, *reopened.master);
        assert_eq!(first.binding, reopened.binding);
        assert_ne!(first.root_id, first.key_id);
        assert_ne!(*first.master, [3; 32]);
        for domain in [b"mdk".as_slice(), b"journal"] {
            let mut other = Zeroizing::new([0; 32]);
            Hkdf::<Sha256>::new(Some(b"marmish/v2/storage"), &[3; 32])
                .expand(domain, other.as_mut())
                .unwrap();
            assert_ne!(*first.master, *other);
        }
        let other_profile = derive(&"02".repeat(16), [2; 32], &[3; 32]).unwrap();
        let other_account = derive(&profile, [4; 32], &[3; 32]).unwrap();
        assert_ne!(first.binding, other_profile.binding);
        assert_ne!(first.binding, other_account.binding);
        assert_ne!(
            *first.master,
            *derive(&profile, [2; 32], &[5; 32]).unwrap().master
        );
        assert!(derive("invalid", [2; 32], &[3; 32]).is_err());
    }
}
