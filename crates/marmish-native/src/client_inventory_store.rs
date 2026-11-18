//! Private per-grant access evidence on the profile's SQLCipher connection.
use super::{JOURNAL_FORMAT, Journal, db, sql_optional_u64};
use crate::command_wire::Account;
use crate::{Error, Result};
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_CONFIGURED_ACCOUNTS: usize = 4096;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ClientInventorySubmitState {
    NeverRecorded,
    Recorded,
    Invalidated,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ClientInventoryAccess {
    pub(crate) last_authenticated_submit_at: Option<u64>,
    pub(crate) last_engine_admitted_submit_at: Option<u64>,
    pub(crate) authenticated_submit_state: ClientInventorySubmitState,
    pub(crate) wall_clock_regressed: bool,
}

pub(super) fn inventory_init_in(tx: &Transaction<'_>, old_format: i64) -> Result<()> {
    if old_format < JOURNAL_FORMAT {
        tx.execute_batch(
            "ALTER TABLE command_invocations ADD COLUMN inventory_generation BLOB
               CHECK(inventory_generation IS NULL OR length(inventory_generation)=16);
             CREATE TABLE client_inventory(
               account BLOB NOT NULL PRIMARY KEY CHECK(length(account)=32),
               grant_generation BLOB NOT NULL CHECK(length(grant_generation)=16),
               submit_cursor INTEGER CHECK(submit_cursor>0),
               submit_at INTEGER CHECK(submit_at>=0),
               submit_group BLOB,
               submit_native_id BLOB,
               admitted_cursor INTEGER CHECK(admitted_cursor>0),
               admitted_at INTEGER CHECK(admitted_at>=0),
               receipt_high_water INTEGER CHECK(receipt_high_water>=0),
               CHECK((submit_at IS NULL AND submit_group IS NULL AND submit_native_id IS NULL)
                 OR (submit_at IS NOT NULL AND submit_cursor IS NOT NULL
                   AND submit_group IS NOT NULL AND submit_native_id IS NOT NULL)),
               CHECK((admitted_cursor IS NULL)=(admitted_at IS NULL))
             );
             CREATE INDEX client_inventory_source
               ON client_inventory(submit_group,submit_native_id)
               WHERE submit_group IS NOT NULL;",
        )
        .map_err(|_| Error::Storage("client_inventory_migration"))?;
    }
    // A file claiming this format must already have the required schema.
    for sql in [
        "SELECT inventory_generation FROM command_invocations LIMIT 0",
        "SELECT account,grant_generation,submit_cursor,submit_at,submit_group,submit_native_id,
                admitted_cursor,admitted_at,receipt_high_water
         FROM client_inventory INDEXED BY client_inventory_source
         WHERE submit_group IS NOT NULL LIMIT 0",
    ] {
        tx.prepare(sql)
            .map_err(|_| Error::Storage("client_inventory_schema"))?;
    }
    Ok(())
}

impl Journal {
    pub(crate) fn reconcile_client_inventory(
        &mut self,
        configured: &BTreeSet<Account>,
    ) -> Result<()> {
        if configured.len() > MAX_CONFIGURED_ACCOUNTS {
            return Err(Error::Invalid("client_inventory_capacity"));
        }
        let tx = self.conn.transaction().map_err(db)?;
        let existing = {
            let mut query = tx
                .prepare("SELECT account FROM client_inventory LIMIT ?1")
                .map_err(db)?;
            query
                .query_map([(MAX_CONFIGURED_ACCOUNTS + 1) as i64], |row| {
                    row.get::<_, Vec<u8>>(0)?
                        .try_into()
                        .map_err(|_| rusqlite::Error::InvalidQuery)
                })
                .map_err(db)?
                .collect::<rusqlite::Result<BTreeSet<Account>>>()
                .map_err(db)?
        };
        if existing.len() > MAX_CONFIGURED_ACCOUNTS {
            return Err(Error::Storage("client_inventory_capacity"));
        }
        for account in existing.difference(configured) {
            tx.execute(
                "DELETE FROM client_inventory WHERE account=?1",
                [account.as_slice()],
            )
            .map_err(db)?;
        }
        for account in configured.difference(&existing) {
            let mut generation = [0_u8; 16];
            getrandom::fill(&mut generation).map_err(|_| Error::Identity("random"))?;
            tx.execute(
                "INSERT INTO client_inventory(account,grant_generation) VALUES(?1,?2)",
                params![account.as_slice(), generation.as_slice()],
            )
            .map_err(db)?;
        }
        tx.commit().map_err(db)
    }

    pub(crate) fn client_inventory_access(
        &self,
        account: &Account,
    ) -> Result<ClientInventoryAccess> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| Error::Invalid("clock"))?
            .as_secs();
        self.client_inventory_access_at(account, now)
    }

    fn client_inventory_access_at(
        &self,
        account: &Account,
        now: u64,
    ) -> Result<ClientInventoryAccess> {
        self.conn
            .query_row(
                "SELECT submit_cursor,submit_at,admitted_at,receipt_high_water
                 FROM client_inventory WHERE account=?1",
                [account.as_slice()],
                |row| {
                    let submit_cursor = sql_optional_u64(row, 0)?;
                    let submit_at = sql_optional_u64(row, 1)?;
                    let admitted_at = sql_optional_u64(row, 2)?;
                    let high_water = sql_optional_u64(row, 3)?;
                    let authenticated_submit_state = match (submit_cursor, submit_at) {
                        (Some(_), Some(_)) => ClientInventorySubmitState::Recorded,
                        (Some(_), None) => ClientInventorySubmitState::Invalidated,
                        (None, None) => ClientInventorySubmitState::NeverRecorded,
                        (None, Some(_)) => return Err(rusqlite::Error::InvalidQuery),
                    };
                    Ok(ClientInventoryAccess {
                        last_authenticated_submit_at: submit_at,
                        last_engine_admitted_submit_at: admitted_at,
                        authenticated_submit_state,
                        wall_clock_regressed: high_water.is_some_and(|high| {
                            now < high || submit_at.is_some_and(|receipt| receipt < high)
                        }),
                    })
                },
            )
            .map_err(|_| Error::Storage("client_inventory_access"))
    }
}

pub(super) fn record_submit(
    tx: &Transaction<'_>,
    invocation: &[u8],
    account: &Account,
    group: &[u8],
    native_id: &[u8],
) -> Result<()> {
    let invocation = tx
        .query_row(
            "UPDATE command_invocations
             SET inventory_generation=(SELECT grant_generation FROM client_inventory WHERE account=?2)
             WHERE k=?1 AND EXISTS(SELECT 1 FROM client_inventory WHERE account=?2)
               AND NOT EXISTS(SELECT 1 FROM command_early_invalidations
                              WHERE group_id=?3 AND native_id=?4)
             RETURNING cursor,created",
            params![invocation, account.as_slice(), group, native_id],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
        )
        .optional()
        .map_err(db)?;
    if let Some((cursor, receipt)) = invocation {
        tx.execute(
            "UPDATE client_inventory
             SET submit_cursor=?2,submit_at=?3,submit_group=?4,submit_native_id=?5,
                 receipt_high_water=max(coalesce(receipt_high_water,?3),?3)
             WHERE account=?1 AND (submit_cursor IS NULL OR submit_cursor<?2)",
            params![account.as_slice(), cursor, receipt, group, native_id],
        )
        .map_err(db)?;
    }
    Ok(())
}

pub(super) fn record_admission(
    tx: &Transaction<'_>,
    invocation: &[u8],
    account: &Account,
) -> Result<()> {
    tx.execute(
        "UPDATE client_inventory
         SET (admitted_cursor,admitted_at)=(SELECT cursor,created FROM command_invocations WHERE k=?2)
         WHERE account=?1 AND EXISTS (
           SELECT 1 FROM command_invocations AS i
           WHERE i.k=?2 AND i.inventory_generation=client_inventory.grant_generation
             AND (admitted_cursor IS NULL OR admitted_cursor<i.cursor))",
        params![account.as_slice(), invocation],
    )
    .map_err(db)?;
    Ok(())
}

pub(super) fn invalidate_source(conn: &Connection, group: &[u8], native_id: &[u8]) -> Result<()> {
    conn.execute(
        "UPDATE client_inventory SET submit_at=NULL,submit_group=NULL,submit_native_id=NULL
         WHERE submit_group=?1 AND submit_native_id=?2",
        params![group, native_id],
    )
    .map_err(db)?;
    Ok(())
}

pub(super) fn invalidate_group(conn: &Connection, group: &[u8]) -> Result<()> {
    conn.execute(
        "UPDATE client_inventory SET submit_at=NULL,submit_group=NULL,submit_native_id=NULL
         WHERE submit_group=?1",
        [group],
    )
    .map_err(db)?;
    Ok(())
}

#[cfg(test)]
mod core_tests {
    use super::*;
    use crate::command_wire::{ErrorCode, State, WireError};
    use crate::journal::command_store::tests::{
        NOW, admitted, fact, owner, projection, publication, response, setup, submit,
    };
    use crate::journal::{AdmissionDecision, CommandNativeUpdate};

    #[test]
    fn upgrades_encrypted_format_seven_and_refuses_incomplete_format_eight() {
        let (dir, mut journal) = setup();
        let account = owner().binding().client;
        let (_, prior_key) = admitted(&mut journal, 1);
        journal
            .conn
            .execute_batch(
                "DROP TABLE client_inventory;
             ALTER TABLE command_invocations DROP COLUMN inventory_generation;
             PRAGMA user_version=7;",
            )
            .unwrap();
        drop(journal);
        let path = dir.path().join("journal.db");
        let profile = hex::encode([2; 32]);
        assert!(Journal::open(&path, &[9; 32], &profile).is_err());
        let mut journal = Journal::open(&path, &[7; 32], &profile).unwrap();
        journal
            .reconcile_client_inventory(&BTreeSet::from([account]))
            .unwrap();
        assert_eq!(
            journal
                .client_inventory_access(&account)
                .unwrap()
                .authenticated_submit_state,
            ClientInventorySubmitState::NeverRecorded,
        );
        fact(&mut journal, &prior_key, State::Running);
        assert_eq!(
            journal
                .client_inventory_access(&account)
                .unwrap()
                .last_engine_admitted_submit_at,
            None
        );
        journal
            .conn
            .execute("DROP INDEX client_inventory_source", [])
            .unwrap();
        assert!(matches!(
            Journal::open(&path, &[7; 32], &profile),
            Err(Error::Storage("client_inventory_schema"))
        ));
    }

    #[test]
    fn fresh_and_quota_refused_receipts_are_distinct_from_engine_admission() {
        let (_, mut journal) = setup();
        let account = owner().binding().client;
        journal
            .reconcile_client_inventory(&BTreeSet::from([account, [3; 32]]))
            .unwrap();
        assert_eq!(
            journal
                .client_inventory_access(&account)
                .unwrap()
                .last_authenticated_submit_at,
            None
        );
        let (mut original, key) = admitted(&mut journal, 1);
        fact(&mut journal, &key, State::Running);
        original.decision = AdmissionDecision::Existing;
        original.response = None;
        journal.command_project_request(&original, NOW + 1).unwrap();
        let mut quota = projection(submit(2));
        let error = WireError {
            code: ErrorCode::QuotaExceeded,
            detail: None,
        };
        let mut refused = response(&quota.request);
        refused.result = Err(error.clone());
        quota.response = Some(publication(&quota.owner, &refused));
        quota.decision = AdmissionDecision::Refused(error);
        journal.command_project_request(&quota, NOW + 2).unwrap();
        let evidence = journal
            .client_inventory_access_at(&account, NOW + 2)
            .unwrap();
        assert_eq!(evidence.last_authenticated_submit_at, Some(NOW + 2));
        assert_eq!(evidence.last_engine_admitted_submit_at, Some(NOW));
        assert_eq!(
            evidence.authenticated_submit_state,
            ClientInventorySubmitState::Recorded
        );
        assert_eq!(
            serde_json::to_value(evidence).unwrap(),
            serde_json::json!({
                "last_authenticated_submit_at": NOW+2,
                "last_engine_admitted_submit_at": NOW,
                "authenticated_submit_state": "recorded",
                "wall_clock_regressed": false,
            })
        );
        assert_eq!(
            journal
                .client_inventory_access(&[3; 32])
                .unwrap()
                .last_authenticated_submit_at,
            None
        );
    }

    #[test]
    fn only_exact_source_and_room_retract_and_receipt_order_survives_clock_rollback() {
        let (_, mut journal) = setup();
        let account = owner().binding().client;
        journal
            .reconcile_client_inventory(&BTreeSet::from([account]))
            .unwrap();
        let (first, key) = admitted(&mut journal, 1);
        fact(&mut journal, &key, State::Running);
        let mut alternate = projection(first.request.clone());
        alternate.source.native_id.push(14);
        alternate.decision = AdmissionDecision::Existing;
        alternate.response = None;
        journal
            .command_project_request(&alternate, NOW + 1)
            .unwrap();
        for (group, id) in [
            (b"wrong-room".to_vec(), first.source.native_id.clone()),
            (alternate.source.group, alternate.source.native_id),
        ] {
            journal
                .command_apply_native(&[CommandNativeUpdate::Invalidated {
                    group,
                    native_id: id,
                    possible_publication: None,
                }])
                .unwrap();
            assert_eq!(
                journal
                    .client_inventory_access(&account)
                    .unwrap()
                    .last_authenticated_submit_at,
                Some(NOW)
            );
        }
        journal
            .command_apply_native(&[CommandNativeUpdate::Invalidated {
                group: first.source.group.clone(),
                native_id: first.source.native_id,
                possible_publication: None,
            }])
            .unwrap();
        let invalidated = journal.client_inventory_access(&account).unwrap();
        assert_eq!(
            invalidated.authenticated_submit_state,
            ClientInventorySubmitState::Invalidated
        );
        assert_eq!(invalidated.last_authenticated_submit_at, None);
        assert_eq!(invalidated.last_engine_admitted_submit_at, Some(NOW));
        journal
            .command_project_request(&projection(submit(2)), NOW - 1)
            .unwrap();
        let regressed = journal
            .client_inventory_access_at(&account, NOW + 1)
            .unwrap();
        assert_eq!(regressed.last_authenticated_submit_at, Some(NOW - 1));
        assert!(regressed.wall_clock_regressed);
        journal.command_revoke_group(&first.source.group).unwrap();
        assert_eq!(
            journal
                .client_inventory_access(&account)
                .unwrap()
                .authenticated_submit_state,
            ClientInventorySubmitState::Invalidated
        );
    }

    #[test]
    fn early_invalidation_and_late_completion_cannot_replace_newer_evidence() {
        let (_, mut journal) = setup();
        let account = owner().binding().client;
        journal
            .reconcile_client_inventory(&BTreeSet::from([account]))
            .unwrap();
        let (older, first) = admitted(&mut journal, 1);
        let candidate = projection(submit(2));
        journal
            .command_apply_native(&[CommandNativeUpdate::Invalidated {
                group: candidate.source.group.clone(),
                native_id: candidate.source.native_id.clone(),
                possible_publication: Some(older.response.unwrap().key),
            }])
            .unwrap();
        let rejected = journal
            .command_project_request(&candidate, NOW + 1)
            .unwrap()
            .key;
        fact(&mut journal, &rejected, State::Running);
        let original = journal.client_inventory_access(&account).unwrap();
        assert_eq!(original.last_authenticated_submit_at, Some(NOW));
        assert_eq!(original.last_engine_admitted_submit_at, None);

        let newest = journal
            .command_project_request(&projection(submit(3)), NOW + 2)
            .unwrap()
            .key;
        fact(&mut journal, &newest, State::Running);
        fact(&mut journal, &first, State::Running);
        let latest = journal.client_inventory_access(&account).unwrap();
        assert_eq!(latest.last_authenticated_submit_at, Some(NOW + 2));
        assert_eq!(latest.last_engine_admitted_submit_at, Some(NOW + 2));
    }

    #[test]
    fn non_submit_and_nonqualifying_refusals_leave_history_unchanged() {
        use crate::command_wire::RequestOp;

        let (_, mut journal) = setup();
        let account = owner().binding().client;
        journal
            .reconcile_client_inventory(&BTreeSet::from([account]))
            .unwrap();
        let (original, _) = admitted(&mut journal, 1);
        let expected = journal.client_inventory_access(&account).unwrap();
        for code in [
            ErrorCode::Busy,
            ErrorCode::FutureDeferred,
            ErrorCode::ClockBehindFloor,
            ErrorCode::StorageFailure,
            ErrorCode::TransportDegraded,
            ErrorCode::InvocationConflict,
            ErrorCode::NotFound,
        ] {
            let mut request = submit(2);
            if code == ErrorCode::NotFound {
                request.op = RequestOp::Status {
                    invocation: request.op.invocation(),
                };
            }
            let mut plan = projection(request);
            let error = WireError { code, detail: None };
            let mut refused = response(&plan.request);
            refused.result = Err(error.clone());
            plan.response = Some(publication(&plan.owner, &refused));
            plan.decision = AdmissionDecision::Refused(error);
            let result = journal.command_project_request(&plan, NOW + 1);
            assert_eq!(result.is_ok(), code == ErrorCode::NotFound);
            assert_eq!(journal.client_inventory_access(&account).unwrap(), expected);
        }

        let mut conflict = submit(3);
        if let RequestOp::Submit(ref mut payload) = conflict.op {
            payload.invocation = original.request.op.invocation();
            payload.argv.push(b"conflicting".to_vec());
        }
        let mut plan = projection(conflict);
        let error = WireError {
            code: ErrorCode::InvocationConflict,
            detail: None,
        };
        let mut refused = response(&plan.request);
        refused.result = Err(error.clone());
        plan.response = Some(publication(&plan.owner, &refused));
        plan.decision = AdmissionDecision::Refused(error);
        journal.command_project_request(&plan, NOW + 1).unwrap();
        assert!(
            journal
                .command_project_request(&projection(submit(4)), NOW - 301)
                .is_err()
        );
        assert_eq!(journal.client_inventory_access(&account).unwrap(), expected);
    }

    #[test]
    fn inventory_provenance_survives_command_gc_and_cursor_is_never_reused() {
        use crate::journal::{CommandProtection, SourceKey, SourceKind};

        let (dir, mut journal) = setup();
        let account = owner().binding().client;
        journal
            .reconcile_client_inventory(&BTreeSet::from([account]))
            .unwrap();
        let mut original = projection(submit(1));
        let error = WireError {
            code: ErrorCode::QuotaExceeded,
            detail: None,
        };
        let mut refused = response(&original.request);
        refused.result = Err(error.clone());
        let publication = publication(&original.owner, &refused);
        original.response = Some(publication.clone());
        original.decision = AdmissionDecision::Refused(error);
        let first = journal.command_project_request(&original, NOW).unwrap().key;
        let source = SourceKey {
            group: original.source.group,
            native_id: original.source.native_id,
        };
        journal
            .command_record_native_acks(std::slice::from_ref(&source), &[])
            .unwrap();
        journal
            .command_apply_native(&[CommandNativeUpdate::Failed {
                key: publication.key,
            }])
            .unwrap();
        let later = NOW + 7 * 24 * 60 * 60 + 1;
        journal
            .command_maintain(later, &CommandProtection::default())
            .unwrap();
        assert!(journal.command_get(&first).unwrap().is_none());
        drop(journal);

        let mut journal = Journal::open(
            &dir.path().join("journal.db"),
            &[7; 32],
            &hex::encode([2; 32]),
        )
        .unwrap();
        let page = journal.command_native_bindings(None, 64).unwrap();
        assert_eq!(page.rows.len(), 1);
        let recovered = &page.rows[0];
        assert!(recovered.cursor.kind == SourceKind::Inventory);
        assert!(recovered.acked && recovered.publication.is_none());
        assert!(recovered.source == source);
        let first_cursor = recovered.cursor.row_id;
        journal
            .command_apply_native(&[CommandNativeUpdate::Invalidated {
                group: source.group,
                native_id: source.native_id,
                possible_publication: None,
            }])
            .unwrap();
        assert_eq!(
            journal
                .client_inventory_access(&account)
                .unwrap()
                .authenticated_submit_state,
            ClientInventorySubmitState::Invalidated
        );
        assert!(
            journal
                .command_native_bindings(None, 64)
                .unwrap()
                .rows
                .is_empty()
        );

        let mut next = submit(2);
        next.id[..6].copy_from_slice(&later.to_be_bytes()[2..]);
        let crate::command_wire::RequestOp::Submit(ref mut payload) = next.op else {
            unreachable!()
        };
        payload.invocation[..6].copy_from_slice(&later.to_be_bytes()[2..]);
        journal
            .command_project_request(&projection(next), later)
            .unwrap();
        let new_cursor: u64 = journal
            .conn
            .query_row(
                "SELECT submit_cursor FROM client_inventory WHERE account=?1",
                [account.as_slice()],
                |row| super::super::sql_u64(row, 0),
            )
            .unwrap();
        assert!(new_cursor > first_cursor);
        assert_eq!(
            journal
                .client_inventory_access(&account)
                .unwrap()
                .last_authenticated_submit_at,
            Some(later)
        );
    }

    #[test]
    fn removed_grant_cannot_receive_an_old_completion_after_readdition() {
        let (dir, mut journal) = setup();
        let account = owner().binding().client;
        let configured = BTreeSet::from([account]);
        journal.reconcile_client_inventory(&configured).unwrap();
        let (_, held) = admitted(&mut journal, 1);
        drop(journal);
        let path = dir.path().join("journal.db");
        let profile = hex::encode([2; 32]);
        let open = || Journal::open(&path, &[7; 32], &profile).unwrap();

        let mut journal = open();
        journal.reconcile_client_inventory(&configured).unwrap();
        assert_eq!(
            journal
                .client_inventory_access(&account)
                .unwrap()
                .last_authenticated_submit_at,
            Some(NOW)
        );
        journal
            .reconcile_client_inventory(&BTreeSet::new())
            .unwrap();
        drop(journal);

        let mut journal = open();
        journal.reconcile_client_inventory(&configured).unwrap();
        fact(&mut journal, &held, State::Running);
        let clean = journal.client_inventory_access(&account).unwrap();
        assert_eq!(
            clean.authenticated_submit_state,
            ClientInventorySubmitState::NeverRecorded
        );
        assert_eq!(clean.last_engine_admitted_submit_at, None);
        let (_, current) = admitted(&mut journal, 2);
        fact(&mut journal, &current, State::Running);
        assert_eq!(
            journal
                .client_inventory_access(&account)
                .unwrap()
                .last_engine_admitted_submit_at,
            Some(NOW)
        );
    }
}
