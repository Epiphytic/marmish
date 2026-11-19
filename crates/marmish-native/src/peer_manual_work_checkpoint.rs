//! Read-only work selection for the installed Manual recipient's Unknown slot.

use super::*;
use receiver_inventory_store::{Charge, class_charge};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ManualDue {
    Provision(Selector),
    Resume(Selector),
    Publish(Selector),
    Held(Selector, u64),
    JoinStarted(Selector, u64),
}

impl Checkpoint {
    pub(crate) fn select_manual_due(
        &mut self,
        source: &Source,
        route: (PublicKey, &str, u64),
        expected: Option<Charge>,
    ) -> Result<Option<ManualDue>> {
        let (account, url, wall) = route;
        self.read_installed_consent(source, account, url, wall, |tx, cx| {
            Self::require_manual_charge(tx, cx, expected)?;
            if let Some(charge) = expected {
                occupied_due(tx, cx, charge, wall)
            } else {
                Ok(tx
                    .query_row(FRESH, params![cx.origin, seconds(wall)?], selector)
                    .optional()?
                    .map(ManualDue::Provision))
            }
        })
    }

    pub(crate) fn select_manual_pending(
        &mut self,
        source: &Source,
        route: (PublicKey, &str, u64),
        expected: Charge,
    ) -> Result<Option<(Selector, u64)>> {
        let (account, url, wall) = route;
        self.read_installed_consent(source, account, url, wall, |tx, cx| {
            Self::require_manual_charge(tx, cx, Some(expected))?;
            let Some(CustodyDue::Pending) = custody_due(tx, cx, wall)? else {
                return Ok(None);
            };
            require_sole_pending(tx)?;
            Ok(Some((
                expected.binding.key,
                expected.binding.generation.count,
            )))
        })
    }

    pub(crate) fn successor_live_offer(
        &mut self,
        source: &Source,
        key: Selector,
        wall: u64,
    ) -> Result<wire::Offer> {
        if !self.policy.permits(key.server) {
            return Err(Error::Policy);
        }
        let route = self.routes.first().ok_or(Error::Source)?;
        let [url] = route.urls.as_slice() else {
            return Err(Error::Source);
        };
        let url = url.clone();
        self.read_installed_consent(source, self.signer.public_key(), &url, wall, |tx, cx| {
            let invite = Invitation::load(tx, key, cx.origin)?;
            if wall >= invite.expires {
                return Err(Error::Expired);
            }
            Self::check_signed_offer(&cx.codec, &cx.signer, &cx.route, &invite, wall)
        })
    }

    fn require_manual_charge(
        tx: &Transaction<'_>,
        cx: &Context,
        expected: Option<Charge>,
    ) -> Result<()> {
        if class_charge(tx, cx.origin, false)? != expected {
            return Err(CursorError::Stale.into());
        }
        Ok(())
    }
}

fn selector(row: &rusqlite::Row<'_>) -> rusqlite::Result<Selector> {
    Ok(Selector {
        server: PublicKey::from_byte_array(row.get(0)?),
        id: row.get(1)?,
        revision: row.get(2)?,
    })
}

fn occupied_due(
    tx: &Transaction<'_>,
    cx: &Context,
    charge: Charge,
    wall: u64,
) -> Result<Option<ManualDue>> {
    let Binding {
        key, generation, ..
    } = charge.binding;
    if let Some(custody) = custody_due(tx, cx, wall)? {
        return Ok(match custody {
            CustodyDue::Held => Some(ManualDue::Held(key, generation.count)),
            CustodyDue::JoinStarted => Some(ManualDue::JoinStarted(key, generation.count)),
            CustodyDue::Pending | CustodyDue::Ineligible => None,
        });
    }
    let sendable: Option<bool> = tx
        .query_row(
            "SELECT o.ack_at IS NULL AND o.expires>?1 AND o.package_expires>?1
                AND (o.retry_at IS NULL OR o.retry_at<=?1)
             FROM peer_prepared_consent_outbox o WHERE o.class=0",
            [seconds(wall)?],
            |row| row.get(0),
        )
        .optional()?;
    if let Some(sendable) = sendable {
        return Ok(sendable.then_some(ManualDue::Publish(key)));
    }
    let ready: bool = tx.query_row(
        "SELECT i.reserved=0 AND i.decision_at IS NULL AND i.expires>?1
          AND NOT EXISTS (SELECT 1 FROM peer_retirement_cooldown d
                           WHERE d.server=i.server AND d.until>?1)
         FROM peer_consent_invite i WHERE i.server=?2 AND i.offer_id=?3
            AND i.revision=?4 AND i.origin=?5",
        params![
            seconds(wall)?,
            key.server.to_bytes(),
            key.id,
            key.revision,
            cx.origin
        ],
        |row| row.get(0),
    )?;
    Ok(ready.then_some(ManualDue::Resume(key)))
}

enum CustodyDue {
    Held,
    JoinStarted,
    Pending,
    Ineligible,
}

fn custody_due(tx: &Transaction<'_>, cx: &Context, wall: u64) -> Result<Option<CustodyDue>> {
    // The epoch verifier already enforces the complete slot/outbox coupling.
    // Resolve even an ineligible row here to keep it from becoming another turn.
    let state: Option<i64> = tx
        .query_row(
            "SELECT CASE
                 WHEN c.mode!=0 OR c.held_at>?3 OR EXISTS (
                     SELECT 1 FROM peer_pending_terminal t
                     WHERE t.class=c.class AND t.generation=c.generation
                 ) THEN -1
                 WHEN c.state=0 AND c.held_deadline>?3 THEN 0
                 WHEN c.state=1 AND c.started_at<=?3 AND c.held_deadline>?3 THEN 1
                 WHEN c.state=2 AND c.qualified_at<=?3 AND c.pending_deadline>?3 THEN 2
                 ELSE -1 END
             FROM peer_pending_custody c WHERE c.class=0 AND c.account=?1 AND c.origin=?2",
            params![cx.account, cx.origin, seconds(wall)?],
            |row| row.get(0),
        )
        .optional()?;
    Ok(state.map(|state| match state {
        0 => CustodyDue::Held,
        1 => CustodyDue::JoinStarted,
        2 => CustodyDue::Pending,
        _ => CustodyDue::Ineligible,
    }))
}

pub(super) fn require_sole_pending(tx: &Transaction<'_>) -> Result<()> {
    let pending: i64 = tx.query_row(
        "SELECT count(*) FROM peer_pending_custody WHERE state=2",
        [],
        |row| row.get(0),
    )?;
    if pending != 1 {
        return Err(CursorError::Stale.into());
    }
    Ok(())
}

const FRESH: &str = "
    SELECT i.server,i.offer_id,i.revision FROM peer_consent_invite i
    JOIN peer_installed_offer_v2 m ON m.server=i.server AND m.offer_id=i.offer_id
        AND m.revision=i.revision AND m.origin=i.origin AND m.version=2
    JOIN peer_provisioning_slot p ON p.class=0 AND p.server IS NULL AND p.next_start_at<=?2
    WHERE i.origin=?1 AND i.reserved=0 AND i.decision_at IS NULL AND i.expires>?2
    AND NOT EXISTS (SELECT 1 FROM peer_provisioning_slot other WHERE other.server=i.server)
    AND NOT EXISTS (SELECT 1 FROM peer_prepared_consent_outbox o WHERE o.server=i.server)
    AND NOT EXISTS (SELECT 1 FROM peer_consent_outbox o WHERE o.server=i.server)
    AND NOT EXISTS (SELECT 1 FROM peer_pending_custody c WHERE c.server=i.server)
    AND NOT EXISTS (SELECT 1 FROM peer_retirement_cooldown d WHERE d.server=i.server AND d.until>?2)
    ORDER BY i.server,i.offer_id,i.revision LIMIT 1";

#[cfg(test)]
#[test]
fn manual_due_uses_canonical_offer_coordinates_without_updating_floors() {
    use cgka_traits::app_components::NostrRoutingV1;
    use nostr::event::Event;

    const URL: &str = "wss://manual-work.example/";
    let file = tempfile::NamedTempFile::new().unwrap();
    let open = || HardenedPeerConnection::open(file.path(), [3; 32]).unwrap();
    let signer = Keys::generate();
    let account = signer.public_key();
    const UID: u32 = 123;
    let wall = nostr::types::Timestamp::now().as_secs();
    let routes = NostrRoutingV1::new([3; 32], vec![URL.to_owned()]).unwrap();
    let codec = wire::Codec::for_version(wire::MAX_OUTER_BYTES, ControlVersion::V2).unwrap();
    let mut founders = [Keys::generate(), Keys::generate()];
    founders.sort_by_key(|key| key.public_key().to_bytes());
    let mut predecessor =
        Checkpoint::attach_installed_v2(open(), signer.clone(), UID, URL, Policy::default())
            .unwrap();
    let predecessor_source = predecessor.bind_source(URL).unwrap();
    let mut selectors = Vec::new();
    for (index, founder) in founders.iter().rev().enumerate() {
        let sent = codec
            .prepare_offer(founder, account, [index as u8; 16], &routes, wall)
            .unwrap();
        let event: Event = serde_json::from_slice(sent.frame()).unwrap();
        let raw = RawRecord {
            id: event.id.to_bytes(),
            stamp: event.created_at.as_secs(),
            bytes: sent.frame(),
        };
        assert_eq!(
            predecessor
                .admit(&predecessor_source, raw, wall)
                .unwrap()
                .status,
            Status::Accepted(TrustedOutcome::Applied)
        );
        selectors.push(Selector::from(&sent.flight));
    }
    drop(predecessor);
    let mut cp = Checkpoint::attach_installed_custody(
        open(),
        signer.clone(),
        UID,
        URL,
        Policy::default(),
        ReceivePolicy::Manual,
    )
    .unwrap();
    let source = cp.bind_source(URL).unwrap();
    let exact = selectors[1];
    let floors =
        |cp: &Checkpoint| {
            cp.cursor.conn.query_row(
            "SELECT v.wall,c.wall FROM peer_consent_version v CROSS JOIN peer_cursor_origin c",
            [],
            |r| Ok((r.get::<_, i64>(0)?,r.get::<_, i64>(1)?)),
        ).unwrap()
        };
    let before = floors(&cp);
    assert_eq!(
        cp.select_manual_due(&source, (account, URL, wall + 1), None),
        Ok(Some(ManualDue::Provision(exact)))
    );
    let offer = cp.installed_live_offer(&source, exact, wall + 1).unwrap();
    assert_eq!(Selector::from(&offer), exact);
    assert_eq!(floors(&cp), before);
    assert_eq!(
        cp.select_manual_due(&source, (founders[0].public_key(), URL, wall + 1), None),
        Err(Error::Source)
    );
    assert_eq!(
        cp.select_manual_due(&source, (account, "wss://other.example/", wall + 1), None),
        Err(Error::Source)
    );
    assert_eq!(
        cp.select_manual_due(&source, (account, URL, wall - 1), None),
        Err(CursorError::ClockRegressed.into())
    );

    #[cfg(marmish_peer_receiver_candidate)]
    {
        assert_eq!(
            cp.reserve_manual_package(&source, exact, wall + 1, true, || Err(Error::Source))
                .err(),
            Some(Error::Source)
        );
        assert_eq!(cp.receiver_charges(&source, wall + 1).unwrap()[1], None);
        assert_eq!(floors(&cp), before);
    }
    cp.reserve_provisioning(&source, exact, wall + 1).unwrap();
    let charge = cp.receiver_charges(&source, wall + 1).unwrap()[1].unwrap();
    let occupied = floors(&cp);
    assert_eq!(
        cp.select_manual_due(&source, (account, URL, wall + 2), None),
        Err(CursorError::Stale.into())
    );
    assert_eq!(
        cp.select_manual_due(&source, (account, URL, wall + 2), Some(charge)),
        Ok(Some(ManualDue::Resume(exact)))
    );
    assert_eq!(
        cp.select_manual_pending(&source, (account, URL, wall + 2), charge),
        Ok(None)
    );
    let exclusive = offer.flight().expires_at;
    assert_eq!(
        cp.select_manual_due(&source, (account, URL, exclusive), Some(charge)),
        Ok(None)
    );
    assert_eq!(
        cp.installed_live_offer(&source, exact, exclusive).err(),
        Some(Error::Expired)
    );
    assert_eq!(floors(&cp), occupied);
}
