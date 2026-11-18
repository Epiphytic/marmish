//! One bounded, volatile offer ledger owned by the serialized native supervisor.

use crate::terminal_selection_wire_v2::Ticket;
use std::num::NonZeroU64;
use std::time::Duration;

const PER_CLIENT: usize = 4;
const TOTAL: usize = 32;
const LIFETIME: Duration = Duration::from_secs(90);

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct Scope {
    pub(crate) server: [u8; 32],
    pub(crate) client: [u8; 32],
    // The Command authority digest also commits to the resolved relay tuple.
    pub(crate) command: [u8; 32],
    pub(crate) generation: u64,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct Writer {
    pub(crate) slot: [u8; 16],
    pub(crate) claim: [u8; 16],
    pub(crate) generation: NonZeroU64,
}

struct Entry {
    scope: Scope,
    writer: Writer,
    ticket: Ticket,
    expiry: Duration,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MintError {
    Capacity,
    Invalid,
}

#[derive(Default)]
pub(crate) struct Ledger(Vec<Entry>);

impl Ledger {
    /// The owner supplies a current writer predicate over its own serialized state.
    pub(crate) fn prune(&mut self, now: Duration, live: impl Fn(Scope, Writer) -> bool) {
        self.0
            .retain(|entry| now < entry.expiry && live(entry.scope, entry.writer));
    }

    pub(crate) fn mint(
        &mut self,
        scope: Scope,
        writer: Writer,
        nonce: [u8; 16],
        now: Duration,
    ) -> Result<Ticket, MintError> {
        let ticket = Ticket::new(nonce, writer.slot, writer.claim, writer.generation)
            .ok_or(MintError::Invalid)?;
        let expiry = now.checked_add(LIFETIME).ok_or(MintError::Invalid)?;
        self.0.retain(|entry| now < entry.expiry);
        if self.0.iter().any(|entry| entry.ticket == ticket) {
            return Err(MintError::Invalid);
        }
        let next = Entry {
            scope,
            writer,
            ticket,
            expiry,
        };
        if let Some(entry) = self
            .0
            .iter_mut()
            .find(|entry| entry.scope == scope && entry.writer == writer)
        {
            *entry = next;
            return Ok(ticket);
        }
        if self.0.len() >= TOTAL
            || self
                .0
                .iter()
                .filter(|entry| entry.scope.client == scope.client)
                .count()
                >= PER_CLIENT
        {
            return Err(MintError::Capacity);
        }
        self.0.push(next);
        Ok(ticket)
    }

    /// Effects still require the owner's authority and current writer checks in the same turn.
    pub(crate) fn consume(
        &mut self,
        scope: Scope,
        writer: Writer,
        ticket: Ticket,
        now: Duration,
    ) -> bool {
        let Some(index) = self.0.iter().position(|entry| {
            now < entry.expiry
                && entry.scope == scope
                && entry.writer == writer
                && entry.ticket == ticket
        }) else {
            return false;
        };
        self.0.swap_remove(index);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scope(client: u8, group: u8) -> Scope {
        Scope {
            server: [1; 32],
            client: [client; 32],
            command: [group; 32],
            generation: 1,
        }
    }

    fn writer(slot: u8) -> Writer {
        Writer {
            slot: [slot; 16],
            claim: [10; 16],
            generation: NonZeroU64::MIN,
        }
    }

    #[test]
    fn terminal_offers_core_expires_and_consumes_exact_live_writer_once() {
        let mut ledger = Ledger::default();
        let (scope, writer) = (scope(2, 3), writer(4));
        let now = Duration::from_secs(15);
        let old = ledger.mint(scope, writer, [11; 16], now).unwrap();
        let live = ledger.mint(scope, writer, [12; 16], now).unwrap();
        assert!(!ledger.consume(scope, writer, old, now));
        let foreign = Scope {
            client: [9; 32],
            ..scope
        };
        let changed = Scope {
            generation: 2,
            ..scope
        };
        let successor = Writer {
            claim: [19; 16],
            ..writer
        };
        assert!(!ledger.consume(foreign, writer, live, now));
        assert!(!ledger.consume(changed, writer, live, now));
        assert!(!ledger.consume(scope, successor, live, now));
        assert!(!ledger.consume(scope, writer, live, now + LIFETIME));
        assert!(!Ledger::default().consume(scope, writer, live, now));
        assert!(ledger.consume(
            scope,
            writer,
            live,
            now + LIFETIME - Duration::from_millis(1)
        ));
        assert!(!ledger.consume(scope, writer, live, now));
    }

    #[test]
    fn terminal_offers_core_capacity_preserves_other_live_offers() {
        let mut ledger = Ledger::default();
        let now = Duration::ZERO;
        let writer = writer(4);
        for client in 2..10 {
            for group in 1..=4 {
                ledger
                    .mint(scope(client, group), writer, [client * 8 + group; 16], now)
                    .unwrap();
            }
        }
        assert_eq!(ledger.0.len(), TOTAL);
        assert!(matches!(
            ledger.mint(scope(2, 5), writer, [15; 16], now),
            Err(MintError::Capacity)
        ));
        assert!(matches!(
            ledger.mint(scope(10, 5), writer, [18; 16], now),
            Err(MintError::Invalid)
        ));
        assert!(matches!(
            ledger.mint(scope(10, 5), writer, [0xd1; 16], now),
            Err(MintError::Capacity)
        ));
        let superseding = ledger.mint(scope(2, 1), writer, [0xff; 16], now).unwrap();
        assert_eq!(ledger.0.len(), TOTAL);
        ledger.prune(now, |scope, _| scope.client != [3; 32]);
        assert_eq!(ledger.0.len(), TOTAL - PER_CLIENT);
        assert!(ledger.consume(scope(2, 1), writer, superseding, now));
        assert!(matches!(
            ledger.mint(scope(4, 5), writer, [21; 16], now),
            Err(MintError::Capacity)
        ));
        ledger.mint(scope(2, 5), writer, [15; 16], now).unwrap();
        ledger.prune(LIFETIME, |_, _| true);
        assert!(ledger.0.is_empty());
    }
}
