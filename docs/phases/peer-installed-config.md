# Installed Peer configuration parser

Add a closed, optional `peer` object to the existing private profile
`config.json`. Loading it validates local intent only. All profiles keep
their current runtime behavior, including those with syntactically valid
Peer intent. The installed owner, enablement controls, platform and relay
qualification, status configuration and egress require separate approval.

This increment is based on native source `c71f59d` and the installed Peer
umbrella design review R2 (Claude Code Auto session
`a0c57eca-bcdc-49ba-999a-724d4dcb5939`). It applies the R2 clarification
that an unlisted founder can enter ordinary Manual Pending in Auto
allow-list mode. This increment does not implement admission.

## Schema

Keep version 1 and all current required fields, owner/mode checks and the
1 MiB read/parse bound. Use a single derived, private raw Peer struct with
closed fields inside `command_config.rs`. Omission of `peer` is equivalent
to an empty object. Explicit null, a non-object, unknown fields and
duplicate fields fail using the existing `config_schema` category. Omitted
optional members use defaults. Explicit null is invalid anywhere in this
section, using the existing `non_null_optional` helper for optional objects
and strings. Use internally tagged receive struct variants `Off {}`,
`Manual {}`, `AutoAny {}` and `AutoFounders { founders }` under
`deny_unknown_fields`, following `peer_pending_ipc::Request`. Serde unit
variants can discard extra fields, so they cannot implement this schema.

| Peer field | Accepted syntax and effect |
| --- | --- |
| `receive` | Closed tagged object with required `mode`: `off` (default), `manual`, `auto_founders` with required `founders` array, or `auto_any`. Only `auto_founders` accepts `founders`. The array has 1–256 distinct complete canonical lowercase `npub` values. |
| `originate` | Boolean, defaults to false. Represents an explicit server-side invitation intent. |
| `relay` | Single explicit direct `wss` URL, an origin as detailed below. |
| `relay_trust` | Scalar exactly `spki-sha256:<64 lowercase hex digits>` or `ca-sha256:<64 lowercase hex digits>`. Check exact ASCII lowercase hex before decoding to a separate typed 32-byte digest for each trust model. |

Require relay and trust together. Require the pair if either direction
is requested. Accept a complete valid pair with both directions Off so
a later disable can retain operator intent without keeping authority.
Keep disable, absence and malformed input distinct. A well-formed explicit
Off accepts no founder list. For client role accept receive and reject
originate. Server accepts originate and rejects any non-Off receive.
Both accepts either, both, or neither. These checks depend on role and
requested directions alone. A missing execution grant, automatic founder
membership, local identity, block state and platform eligibility belong
to the eventual owner. Never add a Command, Terminal or NIP-05 entry from
this object. The parsed allow-list does not grant a connection on its own.

Parse each founder with the NIP-19 public key decoder and require exact
canonical re-encoding, following the test-only `peer_pending_ipc::Selection`
pattern. Accept only the complete typed npub. Reject hex, nsec, prefix,
alias, uppercase/mixed-case, whitespace and duplicate canonical keys.
The existing permissive `command_config::account` keeps its current inputs.
The config check recognizes public keys as identities. Signature, current
authority and self-enrollment checks remain runtime work.

Use the existing `RelayPolicy::parse` URL normalization with a singleton
under `RouteMode::Off` as pure syntax validation, keeping only the normalized
`Url`. A raw syntax guard limits input to an authority with an optional
single trailing slash. Reject a path before parser normalization, including
`/.`, encoded dot segments or doubled slash. Reject percent escapes in
the authority, user info even when empty, fragments and queries (also
empty), whitespace, controls, backslashes, non-`wss`, invalid ports,
trailing-dot or onion hosts. Accept case-equivalent `wss` scheme and
host labels. Normalize them along with default port 443 and
absent/trailing slash to the transport URL serialization with a single
root slash. An explicit valid nondefault port stays exact. Use existing
transport normalization for bracketed IP and numeric IPv4 spellings,
retaining only the canonical URL so alternate spellings share one origin. Private addresses and loopback can be syntactically
valid origins. Validation uses no DNS or connection and says nothing about
relay qualification. A CA digest records intent. Locating a private root,
reading certs, enforcing TLS and verifying a qualification record are
separate increments.

Do not use Command aliases, authenticated Offer route strings, public
relay defaults or SRV to fill Peer fields. Do not copy the Peer URL into
the Command relay policy, reuse it as a connector or treat parsed data
as a `QualifiedOrigin`. A valid profile with ordinary `tor=always` still
parses. The deferred Peer owner must treat that route as
`routing_unsupported` and may never use the validated origin as a direct
fallback. Preserve ordinary `auto` or `off` selection exactly. Peer selects
one future direct route independently of the ordinary configured relay
list.

After successful schema decoding and ordinary config checks, validate Peer
semantics in order: role/directions (`config_peer_directions`), founder
array bounds and exact identity (`config_peer_founders`), required relay,
relay syntax/transport-parser errors (`config_peer_relay`), and required
trust and digest validity (`config_peer_relay_trust`). A trust field
without a relay is a missing-relay error. A relay without trust is a
missing-trust error after the relay validates. Never echo an input URL,
fingerprint or principal in diagnostics. Load and setup return the
existing typed configuration error with the Peer reason. Typed future unsupported availability applies to syntactically
valid intent and is not a parser error.

## Code and persistence boundary

1. Add the raw schema and normalized data under `command_config.rs`.
   `RawConfig` optionally deserializes it and `CommandConfig::normalize`
   validates it using the already normalized role. Store normalized intent
   privately on `CommandConfig`, with typed receive mode, canonical founder
   set and a single relay/trust pair. No production accessor until an
   authorized consumer exists. A single documented dead-code allowance,
   scoped to that private data if needed, can be removed by the consumer.
   Keep the connector, daemon, filesystem and existing Command types intact.
2. Exercise `native_setup::proposed` alongside `CommandConfig::from_bytes`.
   Setup already checks raw input via `config_read`, edits the existing
   JSON Value, checks the proposed config, then commits atomically. Reuse it
   unchanged if it preserves the Peer value. Initial setup omits `peer`.
   Existing setup preserves a valid Peer object through a no-op or unrelated
   update. A role change incompatible with preserved directions fails
   validation before a write and does not silently clear intent. Preserve the
   specific Peer-direction reason so setup can name the conflict. Keep
   ordinary `--update` and grant semantics. No writer, migration or new
   flags are needed. Older binaries using a closed schema reject configs
   containing `peer`; remove that object or restore a compatible saved
   config before rollback.
3. Limit source changes to `command_config.rs` and its focused tests, plus
   an existing native setup test module for the preservation regression.
   No dependency, feature, workflow, installer or runtime state change.
   Include this parser seam in the existing trusted core selector set only
   when the core CI change is independently reviewed.

## Acceptance

Run deterministic tests through the same `CommandConfig::from_bytes` entry
used by load and setup. Call the existing setup proposal boundary without
network or real identity. Keep bounded cases and use generated disposable
public keys where needed.

- Existing valid config and generated setup omit Peer and stay Off.
  `peer: {}` and explicit Off are equivalent. Compare ordinary relay,
  trust, execution, Terminal and alias behavior before and after an
  inert valid Peer object.
- Exercise all role and direction combinations, Manual and both explicit
  Auto modes. Reject bare Auto, list with `auto_any`, empty/missing list,
  excess and duplicate founders, wrong NIP-19 type, malformed or
  noncanonical key. Test explicit null, wrong value types, unknown keys
  and duplicates at the top-level Peer key and inside Peer and receive.
  Exercise `receive: {}`, `receive: null` and non-object receive. For each
  of off, manual and auto_any, reject an unknown member, a duplicate
  member, nonempty and empty forbidden founders members.
- Check canonical equivalent spellings including uppercase scheme/host,
  normalized numeric IPv4 and nondefault port retention. Reject the unsafe/noncanonical components above, path normalization
  tricks, unpaired relay/trust, wrong trust discriminator and malformed
  digest. Check an exact Peer origin can differ from Command's, and that
  forced Tor preserves its ordinary policy with Peer intent inert. Check
  semantic error precedence for an incompatible role plus bad founders
  and for a malformed relay plus trust.
- Setup preserves the parsed Peer value under a harmless existing update,
  leaves it absent on a new profile and rejects an incompatible role change
  before the writer. Reuse the existing owner-checked load and total-byte
  tests without loosening their limits.
- Run the new exact test selectors, the existing config and setup suites,
  and strict native all-target Clippy. Record selectors with nonzero case
  counts, `git diff --check` and scoped format results. Test-only layout,
  absence of a production consumer and the exact reviewed diff establish
  no newly reachable Peer effects in this increment.
