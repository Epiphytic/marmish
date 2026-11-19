use super::*;
use rcgen::{CertificateParams, KeyPair};
use std::{
    ffi::CString,
    fs::{self, Permissions},
    os::unix::{ffi::OsStrExt, fs::PermissionsExt},
};

fn args(values: &[&str]) -> impl Iterator<Item = OsString> {
    values.iter().map(OsString::from)
}

#[test]
fn private_arguments_require_explicit_canonical_relay_policy() {
    let base = [
        "/installed/marmishd",
        "/profile",
        "one",
        "wss://127.0.0.1:9443/",
        "off",
    ];
    assert!(matches!(
        arguments(args(&base)).unwrap().intent,
        QualificationIntent::CommandOnly
    ));
    assert!(matches!(
        arguments(args(&["--passive-recipient"]).chain(args(&base)))
            .unwrap()
            .intent,
        QualificationIntent::PassiveRecipient
    ));
    assert!(matches!(
        arguments(args(&["--manual-recipient"]).chain(args(&base)))
            .unwrap()
            .intent,
        QualificationIntent::ManualRecipient
    ));
    let debug_manual =
        arguments(args(&["--debug", "--manual-recipient"]).chain(args(&base))).unwrap();
    assert!(debug_manual.debug);
    assert!(matches!(
        debug_manual.intent,
        QualificationIntent::ManualRecipient
    ));
    assert!(!arguments(args(&base)).unwrap().debug);
    assert!(arguments(args(&["--debug", "--debug"]).chain(args(&base))).is_err());
    assert!(arguments(args(&base).chain(args(&["--debug"]))).is_err());
    assert!(arguments(args(&["--unknown"]).chain(args(&base))).is_err());
    assert!(
        arguments(args(&["--passive-recipient", "--passive-recipient"]).chain(args(&base)))
            .is_err()
    );
    assert!(arguments(args(&base).chain(args(&["--passive-recipient"]))).is_err());
    for flags in [
        ["--manual-recipient", "--manual-recipient"],
        ["--manual-recipient", "--passive-recipient"],
        ["--passive-recipient", "--manual-recipient"],
    ] {
        assert!(arguments(args(&flags).chain(args(&base))).is_err());
    }
    assert!(arguments(args(&base).chain(args(&["--manual-recipient"]))).is_err());
    assert!(arguments(args(&base[..4])).is_err());
    for (index, replacement) in [
        (0, "marmishd"),
        (1, "profile"),
        (2, "../unrelated"),
        (3, "wss://127.0.0.1:0/"),
        (3, "wss://127.0.0.1:9443/?token=secret"),
        (4, "unknown"),
    ] {
        let mut altered = base;
        altered[index] = replacement;
        assert!(arguments(args(&altered)).is_err());
    }
    assert!(arguments(args(&base).chain(args(&["/ca.der"]))).is_ok());
    assert!(arguments(args(&base).chain(args(&["ca.der"]))).is_err());
    assert!(arguments(args(&base).chain(args(&["/ca.der", "extra"]))).is_err());
    for (index, replacement) in [
        (3, "wss://localhost:9443/"),
        (3, "wss://127.0.0.1:9443/non-root"),
        (3, "wss://192.0.2.1:9443/"),
        (4, "always"),
    ] {
        let mut altered = base;
        altered[index] = replacement;
        assert!(matches!(
            arguments(args(&altered).chain(args(&["/ca.der"]))),
            Err(Error::Invalid("qualification_fixture_route"))
        ));
    }
}

#[test]
fn fixture_ca_is_one_bounded_owned_private_der_file() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("ca.der");
    let cert = CertificateParams::default()
        .self_signed(&KeyPair::generate().unwrap())
        .unwrap();
    let expected = cert.der();
    fs::write(&path, expected).unwrap();
    fs::set_permissions(&path, Permissions::from_mode(0o600)).unwrap();
    assert_eq!(read_ca(&path).unwrap().as_ref(), expected.as_ref());
    fs::set_permissions(&path, Permissions::from_mode(0o400)).unwrap();
    assert!(read_ca(&path).is_ok());
    fs::set_permissions(&path, Permissions::from_mode(0o644)).unwrap();
    assert!(read_ca(&path).is_err());
    fs::set_permissions(&path, Permissions::from_mode(0o600)).unwrap();

    let alias = root.path().join("alias");
    fs::hard_link(&path, &alias).unwrap();
    assert!(read_ca(&path).is_err());
    fs::remove_file(&alias).unwrap();
    std::os::unix::fs::symlink(&path, &alias).unwrap();
    assert!(read_ca(&alias).is_err());
    fs::remove_file(&alias).unwrap();

    let fifo = CString::new(alias.as_os_str().as_bytes()).unwrap();
    // SAFETY: a valid NUL-terminated path under the private temporary directory.
    assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0);
    assert!(read_ca(&alias).is_err());

    for body in [
        Vec::new(),
        vec![0; CA_LIMIT as usize + 1],
        [expected.as_ref(), expected.as_ref()].concat(),
    ] {
        fs::write(&path, body).unwrap();
        assert!(read_ca(&path).is_err());
    }
}

#[test]
fn qualification_rejects_unclaimed_or_aliased_daemon_layout() {
    let root = tempfile::tempdir().unwrap();
    let version = root.path().join("versions/unclaimed");
    fs::create_dir_all(&version).unwrap();
    let daemon = version.join("marmishd");
    fs::write(&daemon, b"fixture").unwrap();
    assert!(installed(&daemon).is_err());
    let alias = root.path().join("marmishd");
    std::os::unix::fs::symlink(&daemon, &alias).unwrap();
    assert!(matches!(
        installed(&alias),
        Err(Error::Invalid("qualification_installation"))
    ));
}
