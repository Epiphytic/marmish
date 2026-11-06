# White Noise default design review

Reviewer: Claude Code, claude-sonnet-5-5, permission mode auto.
Scope: docs/phases/whitenoise-default-relays.md and actual native code.

## Round 1

**Verdict: NOT APPROVED.** There are three blocking findings. The scope, ownership split, Tor handling and most of the acceptance list hold up against the code. I ran no tests and made no network calls. I did not verify the Transponder URLs or the SRV claim, because that needs the network.

## Blocking

**1. Defaulting an omitted `relays` field in `RawConfig` is unnecessary and weakens the frozen-authority rule.**
- `RawConfig.relays` is currently required (`command_config.rs:312`). Setup builds the config with `"relays":[]` (`native_setup.rs:133`) and a new profile always writes a concrete list.
- A default on omission therefore adds no feature. It adds only a way for a hand-written or partial config to silently use public relays.
- Such a config also floats with the binary: if a later release changes `default_relays()`, existing omitted-field profiles change authority with no config edit. Mixed-version client and daemon builds would then disagree, producing `destination_policy_mismatch`.
- `native_setup::config_read` and `proposed` operate on the raw JSON `Value`, so an update would never write the field back.
- **Fix:** drop the `RawConfig` default so an omitted `relays` stays `config_schema`. Use `default_relays()` only in `native_setup::proposed`, and only when the profile is new and `o.relays` is `None`.
  - Keep `"relays":[]` out of the validated path.
  - Add tests that an omitted field is rejected, and that `null` and `[]` are rejected.
- If you want omission support anyway, the doc must state the version-float, and setup must write the concrete pair on any write.

**2. Bare-npub versus alias precedence is unspecified, and it can send a command to the wrong remote.**
- Today the no-host branch (`command_target.rs:~95`) looks up an alias directly, with no reserved-prefix guard. The `account()` helper at `command_target.rs:29` has that guard for the `-u` and `npub@host` forms.
- `alias_name` (`command_config.rs:435`) accepts names like `npub1abc…`, so a config can hold an alias spelled as a valid npub but pointing at a different key.
- Once the no-host branch accepts npubs, which one wins is a security decision. The doc's "alias precedence/conflicts" doesn't say.
- **Fix:** state that any token starting with `npub`, `nsec` or `nprofile`, or that is 64-hex, never consults aliases. A valid `npub1` is the identity and anything else in that family is `invalid_remote_identity`. This matches `-u`.
  - Add a test with an alias named exactly like a valid npub that targets a different key. The npub must win with unchanged behaviour for ordinary aliases.
  - Say explicitly that this changes the previous bare-lookup behaviour for such a name.

**3. The quickstart fixture plan breaks the PTY driver and removes its no-network guarantee.**
- `tools/marmish_native_quickstart_check.py:21-26` matches the exact prompt bytes `Explicit relay URL(s), separated by spaces: `. The design rewrites that prompt, so the existing fixture, and any run against the pinned v0.2.3 binary (`scripts/install.sh:9`), would fail to match.
- The fixture uses `relay.invalid` so nothing can reach the network. A blank-relay case writes real White Noise relays into the config.
- **Fix:** keep the relay prompt line byte-identical and print the "Enter keeps existing settings, or uses White Noise for a new profile" text as a separate line before it.
  - Add the blank-relay case as a new fixture row.
  - Assert the written config equals the default pair.
  - Assert no step after setup opens a relay connection.
  - Run it only against the candidate build, never the pinned 0.2.3 binary.

## Non-blocking, but add to the doc

- **Privacy disclosure:**
  - A fresh profile now defaults to `tor=auto`, which connects directly to non-onion relays. A new default therefore sends the client's IP and metadata to a third party.
  - The README and the setup prompt should say so, and name `--tor always` as the alternative.
  - README line 55 ("No public relay is selected automatically") and `agent-spec.md:145` must be rewritten.
  - `docs/phases/installer-quickstart.md:40` and `v0.2-phase2-destinations.md:74` should be marked superseded.
- **Diagnostic commands:** state that `foundation-check`, `command-check` (`cli.rs:341,353`), and `init`/`daemon` with no config do not get the default and still fail without explicit relays.
- **Interactive tor prompt:** a blank Tor answer sets `"auto"` explicitly (`native_setup.rs:223-235`). On an existing `off` profile that already produces `config_exists`. This is pre-existing, so the doc should note it or fix it, and not claim that "Enter keeps existing settings" in general.
- **Tests to add to the acceptance list:**
  - Assert `default_relays()` against the canonical `wss://…/` form and `eu` before `us` order.
  - Parse it through `RelayPolicy::parse` in Off, Auto and Always.
  - Add the bare-npub filter case for `commands`, `wait` and `output` (the `target_filter` path).

I'll re-review once items 1–3 are reflected in the doc.

## Round 2

**APPROVED.** All three prior blockers are resolved. I checked each against the code, and I found no blocking issues.

- **RawConfig default dropped.** `relays: Vec<String>` at `command_config.rs:309` has no `#[serde(default)]`, so missing relays fail today. The design keeps that behaviour. It also keeps `cwd` and the other required fields as they are.
- **Valid npub wins over a same-name alias, with a reserved-prefix guard.** The `account()` helper at `command_target.rs:25` already checks `npub1` first, then the reserved prefixes and 64-hex, and only then consults aliases. The design extends that same order to the bare no-host branch at `command_target.rs:76`. That branch currently goes straight to alias lookup, so it needs the change.
- **PTY prompt and candidate-only blank row.** The prompt at `tools/marmish_native_quickstart_check.py:22` and `native_setup.rs:218` stays byte-exact. Blank input leaves `o.relays` as `None`. The seed config at `native_setup.rs:133` currently uses `"relays":[]`, which the design replaces with the concrete pair for a new profile only. That seed is what lets the offline blank row assert the default pair.

Three non-blocking notes to tighten during implementation:

1. **Alias names beginning `npub`, `nsec` or `nprofile`.**
   - `alias_name()` at `command_config.rs:435` still accepts names like `npub-bob` or `nprofile-work`.
   - Today those names work as bare aliases. After the change the guard rejects them, which contradicts "Existing aliases retain their configured target".
   - I suggest stating this as an accepted narrowing, or noting that the aliases stay reachable through another form. `-u` and inline `npub@` already reject them via `account()`.
2. **Placement of the new explanation line.** Print it before the unchanged relay prompt, or after the blank line is read. Printing it between the prompt and the fixture's expected bytes would break the PTY expect ordering.
3. **Existing unit test.** `both_without_grants_is_disabled_and_no_defaults_for_relays` at `native_setup.rs:445` asserts the old no-default behaviour and must be rewritten. This falls under Agent A's ownership.
