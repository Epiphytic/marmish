//! Admission decisions before consuming a native worker or an offer.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum State {
    Idle,
    Unconfirmed,
    Confirmed,
    Retiring,
    Reserved,
    Quarantined,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum WriterState {
    Unconfirmed,
    Confirmed,
}

impl State {
    /// An uncleared departure counts as pending cleanup even without a link.
    pub(super) fn classify(
        quarantined: bool,
        reserved: bool,
        writer: Option<WriterState>,
        departure_pending: bool,
    ) -> Self {
        match (quarantined, reserved, writer, departure_pending) {
            (true, _, _, _) => Self::Quarantined,
            (_, true, _, _) => Self::Reserved,
            (_, _, Some(WriterState::Confirmed), _) => Self::Confirmed,
            (_, _, Some(WriterState::Unconfirmed), _) => Self::Unconfirmed,
            (_, _, _, true) => Self::Retiring,
            _ => Self::Idle,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum TicketEffect {
    New,
    Observe,
    Takeover,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Intent {
    Regular,
    Observe,
    TakeoverOffer,
    New,
    Ticketed { effect: TicketEffect, matches: bool },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Effect {
    Writer,
    New,
    Observer,
    Takeover,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Outcome {
    Start(Effect),
    Busy,
    Unavailable,
    WriterMissing,
    StaleOffer,
    ReplacementPending,
}

pub(super) fn decide(state: State, intent: Intent) -> Outcome {
    use Intent::{Regular, TakeoverOffer, Ticketed};
    use Outcome::{Busy, ReplacementPending, StaleOffer, Start, Unavailable, WriterMissing};
    use State::{Confirmed, Idle, Quarantined, Reserved, Retiring, Unconfirmed};

    match (state, intent) {
        (_, Intent::New) => Start(Effect::New),
        (Reserved, _) => ReplacementPending,
        (Confirmed, Regular | TakeoverOffer) => Busy,
        (Confirmed, Intent::Observe) => Start(Effect::Observer),
        (
            Confirmed,
            Ticketed {
                effect,
                matches: true,
            },
        ) => Start(match effect {
            TicketEffect::New => Effect::New,
            TicketEffect::Observe => Effect::Observer,
            TicketEffect::Takeover => Effect::Takeover,
        }),
        (_, Ticketed { .. }) => StaleOffer,
        (Quarantined, _) | (Unconfirmed | Retiring, Regular) => Unavailable,
        (Idle, Regular) => Start(Effect::Writer),
        (Idle | Unconfirmed | Retiring, _) => WriterMissing,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn core_tests_terminal_live_slot_classification_precedence() {
        use State::*;

        for (quarantined, reserved, writer, links, expected) in [
            (true, true, Some(WriterState::Confirmed), true, Quarantined),
            (false, true, Some(WriterState::Confirmed), true, Reserved),
            (false, false, Some(WriterState::Confirmed), true, Confirmed),
            (
                false,
                false,
                Some(WriterState::Unconfirmed),
                true,
                Unconfirmed,
            ),
            (false, false, None, true, Retiring),
            (false, false, None, false, Idle),
        ] {
            assert_eq!(
                State::classify(quarantined, reserved, writer, links),
                expected
            );
        }
    }

    #[test]
    fn core_tests_terminal_live_slot_outcome_matrix() {
        use Outcome::*;
        use State::*;

        for (state, regular, observe, takeover, ticket) in [
            (
                Idle,
                Start(Effect::Writer),
                WriterMissing,
                WriterMissing,
                StaleOffer,
            ),
            (
                Unconfirmed,
                Unavailable,
                WriterMissing,
                WriterMissing,
                StaleOffer,
            ),
            (
                Confirmed,
                Busy,
                Start(Effect::Observer),
                Busy,
                Start(Effect::Takeover),
            ),
            (
                Retiring,
                Unavailable,
                WriterMissing,
                WriterMissing,
                StaleOffer,
            ),
            (
                Reserved,
                ReplacementPending,
                ReplacementPending,
                ReplacementPending,
                ReplacementPending,
            ),
            (
                Quarantined,
                Unavailable,
                Unavailable,
                Unavailable,
                StaleOffer,
            ),
        ] {
            assert_eq!(decide(state, Intent::Regular), regular);
            assert_eq!(decide(state, Intent::Observe), observe);
            assert_eq!(decide(state, Intent::TakeoverOffer), takeover);
            assert_eq!(decide(state, Intent::New), Start(Effect::New));
            for (effect, admitted) in [
                (TicketEffect::Observe, Effect::Observer),
                (TicketEffect::Takeover, Effect::Takeover),
                (TicketEffect::New, Effect::New),
            ] {
                let matched = if state == Confirmed {
                    Start(admitted)
                } else {
                    ticket
                };
                let stale = if state == Reserved {
                    ReplacementPending
                } else {
                    StaleOffer
                };
                assert_eq!(
                    decide(
                        state,
                        Intent::Ticketed {
                            effect,
                            matches: true
                        }
                    ),
                    matched
                );
                assert_eq!(
                    decide(
                        state,
                        Intent::Ticketed {
                            effect,
                            matches: false
                        }
                    ),
                    stale
                );
            }
        }
    }
}
