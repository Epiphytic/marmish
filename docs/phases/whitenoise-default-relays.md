# Default White Noise relays

Beads: marmish-i5z. Direct user request supersedes the earlier prohibition on
public defaults: White Noise maintainers have authorized Marmish use.
This is one bounded implementation increment; no version bump or binary release
is part of it. Published v0.2.3 remains unchanged until a later reviewed release.

## Behavior and interfaces

Use the official clear-net pair `wss://relay.eu.whitenoise.chat/` and
`wss://relay.us.whitenoise.chat/`, verified against the official
[Transponder configuration](https://raw.githubusercontent.com/marmot-protocol/transponder/master/config/default.toml)
on 2026-11-05. No usable `_nostr._tcp.whitenoise.chat` or
`_marmot._tcp.whitenoise.chat` SRV answer was observed; do not invent a service
owner, require DNS discovery, or treat discovery failure as fallback authority.

Provide one shared `transport::default_relays()` used by native setup only.
`RawConfig.relays` remains required: missing, null and empty fields fail; an
existing profile never floats with binary defaults. New setup writes
the concrete pair to config, freezing the daemon authority exactly as before.
An explicit empty, null, malformed or unsupported relay selection still fails.
Low-level `RelayPolicy::parse` continues requiring a nonempty explicit list.
Omitted options on setup update preserve the existing configured relay set.
Interactive setup retains its exact relay prompt bytes and prints a separate
line describing Enter as keeping existing relay settings or using White Noise
for a new profile; blank input leaves omission
intact so proposed-config logic makes the decision without replacing settings.
The thin installer delegates setup to the app and needs no duplicate default.

Destination parsing accepts a bare valid npub in the existing no-host branch,
using configured relays when none are selected explicitly. Bare npub plus
`--relay` or `--srv` follows the same validation as existing `-u NPUB` forms.
Ordinary aliases retain their configured target and complete relay set; alias
plus conflicting selectors remains rejected. Malformed/reserved Nostr identity
spellings do not become aliases or hosts. Tokens beginning npub, nsec or
nprofile, and raw 64-hex tokens, never consult aliases; a valid npub identity
wins even over an alias with exactly that name (a deliberate correction to
previous bare-alias lookup). Empty `npub@` remains malformed.
Explicit host/URL/SRV selection and exact configured-set ceiling remain intact.
Neither invalid explicit input nor relay/network failure triggers White Noise.

Daemons and client autostart already inherit normalized profile policy; reuse
that route. Do not implicitly create profiles or grants when daemon starts
without setup. Existing explicit relay profiles and alias sets stay unchanged.
Diagnostic command-check/foundation-check and config-less init/daemon still
require their existing configuration or explicit relay inputs. Interactive blank
Tor input already selects auto; preserving a previous non-auto Tor choice is
out of scope and the relay explanation must not imply otherwise.
README and setup disclose that default Tor auto connects directly to these
public relays; --tor always requests Tor. Mark historical no-default design
statements superseded rather than rewriting their evidence.
No disk format or wire changes; no changes to grants, trust, replay, key custody,
retention, Tor routing or no-direct-fallback rules. Static endpoints avoid
adding clear-net discovery under forced Tor.

## Implementation ownership

Agent A owns `transport.rs`, `command_config.rs`, `native_setup.rs` only: helper,
required-field regression coverage, setup defaults/prompt and focused tests. Agent B owns
`command_target.rs` only: npub selection and destination tests. Root owns docs,
validation/integration and independent Claude review. Avoid shared Cargo jobs;
root runs integrated checks after both agents finish.

## Acceptance

Cover missing vs explicit empty/null relays, correct pair/canonical ordering,
new noninteractive setup, new blank interactive relay choice, preservation on
setup update, explicit custom relays, bare npub/default config, bare npub/custom
config, -u parity, npub with relay/SRV flags, alias precedence/conflicts, invalid
identities, empty inline host, policy mismatch and route modes. Assert a
valid-npub-shaped alias cannot redirect that identity. Cover bare identities
in commands/wait/output target filters. Reuse PTY
quickstart fixture to cover blank relay input without weakening existing prompt,
privacy and cleanup checks; retain explicit-relay fixture checks as well.
The blank case is candidate-only, asserts the concrete default pair and invokes only
offline setup after installation (no network activation is expected; this is
not a packet-capture assertion). Pinned v0.2.3 keeps its existing rows.
Run affected Rust tests and the full native library suite, format and strict
Clippy; run affected Python qualification tests if changing their fixture.
Run a bounded actual direct Marmot command with fresh keys and new setup omitting
relay flags, bare npub destination and byte-exact output. Explicit file custody
and Tor off are acceptable test choices, not changes to product defaults.
Retain finite evidence, joined daemon cleanup and removed private test state.
Update current README/spec while labeling installed v0.2.3 behavior historical.
Obtain independent actual Claude design approval before implementation and
Claude Code auto review after integration, fix useful findings, audit staged
diff/ignore rules, commit/push source to GitHub and Radicle and verify Iris.

## Design approval

Claude Code (claude-sonnet-5-5, auto mode) approved the revised design before
implementation. [Review and dispositions](../reviews/whitenoise-default-design.md).
Reserved-prefix alias names are deliberately no longer usable as aliases; rename
such an alias to a normal name.
