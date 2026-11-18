use super::{Body, BoundSigning, Challenge, Codec, Pair, Prepared, Role, RumorId, Step, Verified};
use crate::{Error, Result};
use nostr::key::{Keys, PublicKey};
use std::time::{Duration, Instant};

const DEADLINE: Duration = Duration::from_secs(30);
const PROBE_INTERVAL: Duration = Duration::from_secs(60);
const AUTHORITY: Error = Error::Invalid("terminal_bootstrap_authority");
const ORDER: Error = Error::Invalid("terminal_bootstrap_order");
const RATE: Error = Error::Invalid("terminal_bootstrap_rate");
const ENTROPY: Error = Error::Invalid("terminal_bootstrap_entropy");

#[derive(Clone, Copy)]
pub(crate) struct Time {
    pub(crate) unix: u64,
    pub(crate) monotonic: Instant,
    pub(crate) continuous: Option<Duration>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Advance {
    Duplicate,
    Send(Step),
    // Availability of an MLS bootstrap transcript, independent of shell grants.
    Confirmed,
}

/// One in-memory exchange and one Probe rate record for a Command binding.
pub(crate) struct Exchange {
    codec: Option<Codec>,
    live: Option<Live>,
    last_probe: Option<Instant>,
}

pub(crate) struct ConfirmedTerminal {
    pub(crate) local: Keys,
    pub(crate) client: PublicKey,
    pub(crate) server: PublicKey,
    pub(crate) ready: RumorId,
    pub(crate) confirmed_at: Time,
}

struct Live {
    began: Instant,
    last: Body,
    ids: [Option<RumorId>; 4],
    sent: [Option<Prepared>; 4],
    local_terminal: Option<Keys>,
    confirmed_at: Option<Time>,
}

impl Live {
    fn probe(began: Instant, body: Body, id: RumorId) -> Self {
        Self {
            began,
            last: body,
            ids: [Some(id), None, None, None],
            sent: [const { None }; 4],
            local_terminal: None,
            confirmed_at: None,
        }
    }

    fn advance(&mut self, id: RumorId, body: Body) {
        self.ids[body.step() as usize] = Some(id);
        self.last = body;
    }

    fn remember(&mut self, prepared: Prepared, body: Body) {
        self.advance(prepared.id, body);
        self.sent[body.step() as usize] = Some(prepared);
    }
}

impl Exchange {
    pub(crate) fn new(codec: Codec) -> Self {
        Self {
            codec: Some(codec),
            live: None,
            last_probe: None,
        }
    }

    // Authority must be checked by the local owner before every transition
    // and retransmission, even when the next message is an exact duplicate.
    fn refresh(&mut self, now: Time, authority: bool) -> Result<()> {
        if !authority {
            self.live = None;
            self.codec = None;
        }
        if self.codec.is_none() {
            return Err(AUTHORITY);
        }
        if self
            .live
            .as_ref()
            .is_some_and(|live| now.monotonic.saturating_duration_since(live.began) >= DEADLINE)
        {
            self.live = None;
        }
        Ok(())
    }

    fn admit_probe(&mut self, now: Time) -> Result<()> {
        if self
            .last_probe
            .is_some_and(|last| now.monotonic.saturating_duration_since(last) < PROBE_INTERVAL)
        {
            return Err(RATE);
        }
        // A valid Probe consumes the rate interval even if generation fails.
        self.last_probe = Some(now.monotonic);
        Ok(())
    }

    pub(crate) fn start(&mut self, signer: &Keys, now: Time, authority: bool) -> Result<()> {
        self.refresh(now, authority)?;
        if self.codec.as_ref().ok_or(AUTHORITY)?.binding.role != Role::Client {
            return Err(ORDER);
        }
        self.admit_probe(now)?;
        let body = Body::Probe {
            client: challenge()?,
        };
        let prepared = self
            .codec
            .as_ref()
            .ok_or(AUTHORITY)?
            .wrap(signer, body, now.unix)?;
        let mut live = Live::probe(now.monotonic, body, prepared.id);
        live.remember(prepared, body);
        self.live = Some(live);
        Ok(())
    }

    pub(crate) fn start_bound(&mut self, signer: &BoundSigning, now: Time) -> Result<()> {
        let keys = signer.for_codec(self.codec.as_ref().ok_or(AUTHORITY)?)?;
        self.start(keys, now, true)
    }

    pub(crate) fn receive_bound(
        &mut self,
        signer: &BoundSigning,
        frame: &[u8],
        now: Time,
        admit: impl FnOnce(Step, RumorId) -> Result<bool>,
    ) -> Result<Advance> {
        let keys = signer.for_codec(self.codec.as_ref().ok_or(AUTHORITY)?)?;
        self.receive_checked(keys, frame, now, true, admit)
    }

    pub(crate) fn receive(
        &mut self,
        signer: &Keys,
        frame: &[u8],
        now: Time,
        authority: bool,
    ) -> Result<Advance> {
        self.receive_checked(signer, frame, now, authority, |_, _| Ok(true))
    }

    /// Invoke admission gates only for a fresh authenticated Probe or the
    /// exact expected Confirm, before a response or terminal key is made.
    pub(crate) fn receive_checked(
        &mut self,
        signer: &Keys,
        frame: &[u8],
        now: Time,
        authority: bool,
        admit: impl FnOnce(Step, RumorId) -> Result<bool>,
    ) -> Result<Advance> {
        self.refresh(now, authority)?;
        let verified = self
            .codec
            .as_ref()
            .ok_or(AUTHORITY)?
            .open(frame, now.unix)?;
        if self
            .live
            .as_ref()
            .is_some_and(|live| live.ids.contains(&Some(verified.id)))
        {
            return Ok(Advance::Duplicate);
        }
        if let Body::Probe { client } = verified.body {
            if !admit(Step::Probe, verified.id)? {
                return Ok(Advance::Duplicate);
            }
            self.admit_probe(now)?;
            let response = Body::Challenge {
                pair: Pair {
                    client,
                    server: challenge()?,
                },
                previous: verified.id,
            };
            let prepared = self
                .codec
                .as_ref()
                .ok_or(AUTHORITY)?
                .wrap(signer, response, now.unix)?;
            let mut live = Live::probe(now.monotonic, verified.body, verified.id);
            live.remember(prepared, response);
            self.live = Some(live);
            return Ok(Advance::Send(Step::Challenge));
        }
        self.advance(signer, verified, now, admit)
    }

    fn advance(
        &mut self,
        signer: &Keys,
        verified: Verified,
        now: Time,
        admit: impl FnOnce(Step, RumorId) -> Result<bool>,
    ) -> Result<Advance> {
        let codec = self.codec.as_ref().ok_or(AUTHORITY)?;
        let live = self.live.as_mut().ok_or(ORDER)?;
        if verified.body.previous() != live.ids[live.last.step() as usize] {
            return Err(ORDER);
        }
        let (response, local_terminal) = match (live.last, verified.body) {
            (Body::Probe { client }, Body::Challenge { pair, .. }) if client == pair.client => {
                let keys = Keys::generate();
                let body = Body::Confirm {
                    pair,
                    previous: verified.id,
                    client_key: keys.public_key(),
                };
                (body, keys)
            }
            (
                Body::Challenge { pair: expected, .. },
                Body::Confirm {
                    pair, client_key, ..
                },
            ) if expected == pair && codec.fresh_terminal_key(client_key) => {
                if !admit(Step::Confirm, verified.id)? {
                    return Ok(Advance::Duplicate);
                }
                let keys = Keys::generate();
                let server_key = keys.public_key();
                if server_key == client_key {
                    return Err(ENTROPY);
                }
                let body = Body::Ready {
                    pair,
                    previous: verified.id,
                    client_key,
                    server_key,
                };
                (body, keys)
            }
            (
                Body::Confirm {
                    pair: expected,
                    client_key: own,
                    ..
                },
                Body::Ready {
                    pair,
                    client_key,
                    server_key,
                    ..
                },
            ) if pair == expected && own == client_key && codec.fresh_terminal_key(server_key) => {
                live.advance(verified.id, verified.body);
                live.confirmed_at = Some(now);
                return Ok(Advance::Confirmed);
            }
            _ => return Err(ORDER),
        };
        if !codec.fresh_terminal_key(local_terminal.public_key()) {
            return Err(ENTROPY);
        }
        let prepared = codec.wrap(signer, response, now.unix)?;
        live.advance(verified.id, verified.body);
        live.remember(prepared, response);
        if response.step() == Step::Ready {
            live.confirmed_at = Some(now);
        }
        live.local_terminal = Some(local_terminal);
        Ok(Advance::Send(response.step()))
    }

    pub(crate) fn retry(&mut self, step: Step, now: Time, authority: bool) -> Result<&Prepared> {
        self.refresh(now, authority)?;
        self.live
            .as_ref()
            .and_then(|live| live.sent[step as usize].as_ref())
            .ok_or(ORDER)
    }

    pub(crate) fn confirmed(
        &mut self,
        now: Time,
        authority: bool,
    ) -> Result<(&Keys, PublicKey, RumorId)> {
        self.refresh(now, authority)?;
        let live = self.live.as_ref().ok_or(ORDER)?;
        let Body::Ready {
            client_key,
            server_key,
            ..
        } = live.last
        else {
            return Err(ORDER);
        };
        let remote = match self.codec.as_ref().ok_or(AUTHORITY)?.binding.role {
            Role::Client => server_key,
            Role::Server => client_key,
        };
        let key = live.local_terminal.as_ref().ok_or(ORDER)?;
        let id = live.ids[Step::Ready as usize].ok_or(ORDER)?;
        Ok((key, remote, id))
    }

    pub(crate) fn take_terminal(
        &mut self,
        now: Time,
        authority: bool,
    ) -> Result<ConfirmedTerminal> {
        let (_, _, ready) = self.confirmed(now, authority)?;
        let live = self.live.as_mut().ok_or(ORDER)?;
        let Body::Ready {
            client_key,
            server_key,
            ..
        } = live.last
        else {
            return Err(ORDER);
        };
        Ok(ConfirmedTerminal {
            local: live.local_terminal.take().ok_or(ORDER)?,
            client: client_key,
            server: server_key,
            ready,
            confirmed_at: live.confirmed_at.ok_or(ORDER)?,
        })
    }

    pub(crate) fn enrollment_codec(&self) -> Result<&Codec> {
        self.codec.as_ref().ok_or(AUTHORITY)
    }
}

impl Codec {
    pub(super) fn fresh_terminal_key(&self, key: PublicKey) -> bool {
        ![
            self.binding.client,
            self.binding.server,
            self.inbound.public,
            self.outbound,
        ]
        .contains(&key)
    }
}

fn challenge() -> Result<Challenge> {
    let mut bytes = [0; 16];
    getrandom::fill(&mut bytes).map_err(|_| ENTROPY)?;
    Ok(bytes)
}
