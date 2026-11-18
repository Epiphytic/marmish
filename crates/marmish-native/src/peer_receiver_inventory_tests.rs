use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, symlink};
use std::path::Path;

use cgka_traits::app_components::NostrRoutingV1;
use nostr::event::Event;

use super::super::provisional::tests::Fixture;
use super::super::store::{
    RawRecord, TrustedOutcome,
    consent_checkpoint::{provisioning::stage_journal::FileWitness, wire},
};
use super::*;

const RELAY: &str = "wss://receiver-inventory.example/";

fn open(fixture: &Fixture) -> RecoveredOwner<'_> {
    RecoveredOwner::open(&fixture.lease, &fixture.identity, RELAY).unwrap()
}

fn write_extent(path: &Path, length: u64) -> FileWitness {
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .unwrap();
    file.set_len(length).unwrap();
    let stat = file.metadata().unwrap();
    FileWitness {
        device: stat.dev().to_be_bytes(),
        inode: stat.ino().to_be_bytes(),
    }
}

#[test]
fn peer_receiver_inventory_charges_unwitnessed_and_interrupted_files_across_reopen() {
    let f = Fixture::new();
    let mut owner = open(&f);
    let fresh = owner.recover(f.now).unwrap();
    assert_eq!(fresh.classes, [Class::Fenced, Class::Vacant]);
    assert_eq!(fresh.charged, RESERVED_BYTES);

    write_extent(&f.stage(), 0);
    assert_eq!(owner.recover(f.now).unwrap().classes, [Class::Fenced; 2]);
    drop(owner);

    let hot = f.stage().with_extension("db-journal");
    write_extent(&hot, 4096);
    let mut owner = open(&f);
    let ambiguous = owner.recover(f.now).unwrap();
    assert_eq!(ambiguous.classes, [Class::Fenced; 2]);
    assert_eq!(ambiguous.charged, RESERVED_BYTES + 4096);
    assert!(hot.exists());
    fs::remove_file(&hot).unwrap();
    fs::remove_file(f.stage()).unwrap();

    let snapshot = f.stage().with_file_name("receiver-join-snapshot.db");
    write_extent(&snapshot, 4096);
    let interrupted = owner.recover(f.now).unwrap();
    assert_eq!(interrupted.classes, [Class::Fenced; 2]);
    assert_eq!(interrupted.extents, [0, 0, 4096]);
    assert!(snapshot.exists());
}

#[test]
fn peer_receiver_inventory_recovers_only_exact_witness_and_retains_expired_slot() {
    let f = Fixture::new();
    let routing = NostrRoutingV1::new([3; 32], vec![RELAY.to_owned()]).unwrap();
    let prepared = wire::Codec::for_version(
        wire::MAX_OUTER_BYTES,
        crate::peer_control_version::ControlVersion::V2,
    )
    .unwrap()
    .prepare_offer(&f.server, f.identity.public_key(), [1; 16], &routing, f.now)
    .unwrap();
    let event: Event = serde_json::from_slice(prepared.frame()).unwrap();
    let record = RawRecord {
        id: event.id.to_bytes(),
        stamp: event.created_at.as_secs(),
        bytes: prepared.frame(),
    };
    let key = Selector::from(&prepared.flight);
    let paths = Paths::new(&f.lease, &f.identity).unwrap();
    let conn =
        HardenedPeerConnection::open_installed(paths.cursor().unwrap().path(), *paths.cursor_key())
            .unwrap();
    let uid = unsafe { libc::geteuid() };
    let mut predecessor = Checkpoint::attach_installed_v2(
        conn,
        (*f.identity.signer()).clone(),
        uid,
        RELAY,
        Policy::default(),
    )
    .unwrap();
    let source = predecessor.bind_source(RELAY).unwrap();
    assert_eq!(
        predecessor.admit(&source, record, f.now).unwrap().status,
        Status::Accepted(TrustedOutcome::Applied),
    );
    drop(predecessor);

    let mut owner = open(&f);
    let reservation = owner
        .checkpoint
        .reserve_provisioning(&owner.source, key, f.now)
        .unwrap();
    let binding = reservation.stage_binding();
    assert_eq!(
        owner.recover(f.now).unwrap().classes[1],
        Class::Create(binding)
    );

    let stage = owner
        .checkpoint
        .bind_stage(&owner.source, &reservation, f.now)
        .unwrap();
    assert_eq!(
        owner.recover(f.now).unwrap().classes[1],
        Class::Create(binding)
    );
    let witness = write_extent(&f.stage(), 4096);
    assert_eq!(owner.recover(f.now).unwrap().classes[1], Class::Fenced);
    owner
        .checkpoint
        .start_stage(&owner.source, &stage, witness, f.now)
        .unwrap();
    let journal = f.stage().with_extension("db-journal");
    let journal_file = write_extent(&journal, 8192);
    let inventory = owner.recover(f.now).unwrap();
    assert_eq!(
        inventory.classes[1],
        Class::JournalCandidate {
            binding,
            journal: FileExtent {
                identity: (
                    u64::from_be_bytes(journal_file.device),
                    u64::from_be_bytes(journal_file.inode),
                ),
                logical: 8192,
                charged: 8192,
            },
        },
    );
    assert_eq!(inventory.charged, RESERVED_BYTES + 12_288);
    assert!(inventory.clean_extent(binding).is_err());
    assert!(owner.recover(f.now - 1).is_err());

    let stale_source = owner.source;
    drop(owner.checkpoint);
    let mut owner = open(&f);
    assert!(
        owner
            .checkpoint
            .receiver_charges(&stale_source, f.now)
            .is_err()
    );
    assert_eq!(owner.recover(f.now + 86_401).unwrap(), inventory);
    let known = f.stage().with_file_name("provisional-known.db");
    write_extent(&known, TOTAL_ADMISSION - inventory.charged + 1);
    let over_budget = owner.recover(f.now).unwrap();
    assert_eq!(over_budget.classes, [Class::Fenced; 2]);
    assert_eq!(over_budget.charged, TOTAL_ADMISSION + 1);
    assert!(journal.exists());
    fs::remove_file(known).unwrap();
    assert_eq!(owner.recover(f.now).unwrap(), inventory);
    fs::remove_file(journal).unwrap();
    assert_clean_charge(&f, &mut owner, binding);

    let staged = OpenOptions::new().read(true).open(f.stage()).unwrap();
    fs::remove_file(f.stage()).unwrap();
    assert_eq!(owner.recover(f.now).unwrap().classes[1], Class::Fenced);
    write_extent(&f.stage(), 4096);
    assert_eq!(owner.recover(f.now).unwrap().classes[1], Class::Fenced);
    drop(staged);
}

fn assert_clean_charge(f: &Fixture, owner: &mut RecoveredOwner<'_>, binding: Binding) {
    let clean = owner.recover(f.now).unwrap().clean_extent(binding).unwrap();
    assert_eq!(clean.main.logical, 4096);
    assert_eq!(clean.charged_other, RESERVED_BYTES + 1);

    let mut stage = OpenOptions::new().write(true).open(f.stage()).unwrap();
    stage.write_all(&[1]).unwrap();
    stage.set_len(1).unwrap();
    stage.sync_all().unwrap();
    let physical = (stage.metadata().unwrap().blocks() * 512).max(1);
    drop(stage);
    let known = f.stage().with_file_name("provisional-known.db");
    write_extent(&known, 8192);
    let clean = owner.recover(f.now).unwrap().clean_extent(binding).unwrap();
    assert_eq!(clean.main.logical, 1);
    assert_eq!(clean.main.charged, physical);
    assert_eq!(clean.charged_other, RESERVED_BYTES + physical + 8192);
    fs::remove_file(known).unwrap();
}

#[test]
fn peer_receiver_inventory_refuses_aliases_unknown_artifacts_and_bad_cursor() {
    let f = Fixture::new();
    let mut owner = open(&f);
    let cursor = f.stage().with_file_name("cursor.db");
    let known = f.stage().with_file_name("provisional-known.db");
    fs::hard_link(&cursor, &known).unwrap();
    assert!(owner.recover(f.now).is_err());
    fs::remove_file(&known).unwrap();
    symlink(&cursor, &known).unwrap();
    assert!(owner.recover(f.now).is_err());
    fs::remove_file(&known).unwrap();
    let unknown = f.stage().with_file_name("unaccounted.db");
    write_extent(&unknown, 4096);
    assert!(owner.recover(f.now).is_err());
    fs::remove_file(unknown).unwrap();

    write_extent(&known, TOTAL_ADMISSION - RESERVED_BYTES);
    let full = owner.recover(f.now).unwrap();
    assert_eq!(full.classes, [Class::Fenced; 2]);
    assert_eq!(full.charged, TOTAL_ADMISSION);
    assert!(known.exists());

    let cursor_journal = cursor.with_extension("db-journal");
    write_extent(&cursor_journal, 0);
    assert!(owner.recover(f.now).is_err());
    drop(owner);
    assert!(RecoveredOwner::open(&f.lease, &f.identity, RELAY).is_err());
    assert!(cursor_journal.exists());
}
