# White Noise default code review

Reviewer: Claude Code, claude-sonnet-5-5, permission mode auto.
Integrated working diff reviewed against approved phase design.

**APPROVED.** I found no blocking issues. I did not run Cargo, per your constraint, so the Rust tests are verified by reading only.

**What I checked against `whitenoise-default-relays.md`**
- **Default pair:** `transport::default_relays()` returns the `wss://…/` pair, EU before US. It is used only in `native_setup::proposed`, for a new profile when `relays` is omitted.
- **Required field:** `RawConfig.relays` is still required. The new test covers missing (`config_schema`), null (`config_schema`) and empty (`relay_count`).
- **Explicit relay lists:** empty, malformed and `ws://` lists still fail, and `RelayPolicy::parse` is untouched. Omitting relays on an existing profile preserves its set, with and without `--update`.
- **Prompt:** The relay prompt bytes are unchanged. The explanation is printed as separate lines before the prompt, so the PTY expect-matching is unaffected.
- **Bare npub parsing:**
  - `parse` (`command_target.rs:78`) runs `account()` first, which applies the length cap, the valid-`npub1` check, the reserved-prefix and 64-hex rejection, and only then alias lookup. A valid npub therefore always wins over a same-name alias.
  - A bare npub takes the same `RelaySource` path as `-u`, so `--relay` and `--srv` behave identically.
  - Ordinary aliases keep their complete relay set and still reject selectors.
  - `npub@` stays malformed.
- **Tests:**
  - Alias precedence, reserved names, SRV with Always, and the `commands`/`wait`/`output` filters are covered, the last via `target_filter` in `cli.rs`.
  - `default_relays()` is parsed in Off, Auto and Always modes.
- **Python:**
  - The default-relays fixture is opt-in through `--default-relays`. It answers only the second dialog entry, with a blank line, and checks `config.json` for the default pair.
  - `interactive(..., dialog=DIALOG)` stays compatible with the keychain and HTTPS helpers that import it.
  - The workflow runs the blank row after the existing explicit-relay row and uploads `quickstart*.json`.
  - `python3 -m unittest test_marmish_quickstart test_marmish_install` ran 52 tests, all OK. It must run without `-I`, because the test modules are imported from the working directory.
- **Docs:** README and spec mark v0.2.3 as historical, disclose the direct connection under Tor `auto`, and name `--tor always`. The two superseded-design banners are present. The alias rename is documented as a deliberate narrowing.

**Non-blocking concerns**
1. **No-connection claim is not enforced.** The design says the blank row "performs no relay connection after offline setup", but `network_activation: False` in `quickstart_check.py` is a hard-coded constant. The earlier `relay.invalid` row would have failed on any attempted connection; the real White Noise relays would not. `MdkOwner::open` appears not to connect, so the claim is probably true today. If you want it enforced, add a post-setup assertion in that row. A cheap one is to assert the profile is unchanged by the repeat `setup --no-grants` and have the CI step block outbound access. Otherwise reword the doc to say "no connection is expected".
2. **`parse` does a double alias lookup.** `account()` already resolves the alias, and then `aliases().get(value)` is called again to get the relays (`command_target.rs:78` and `:85`). Extracting a small `alias_selection(config, value, relays, srvs)` helper removes that branch from `parse`. It also removes the nested `if/else` with the early `return` inside a `match` arm.
3. **Error precedence changed slightly.** A bare unknown name with `--relay` now returns `unknown_alias` rather than `conflicting_destination`. This is harmless, but it is not mentioned in the docs.
4. **No Python unit test covers the new dialog override.** Only CI exercises the `--default-relays` path.
5. **The extra Tor line is printed whenever relays are omitted.** It also shows to someone about to type private relays. This is cosmetic.

**Ripwire `/tmp/marmish-i5z-quality.log`** (7 gating rows)
- **Real, minor:** `parse` complexity went from 27 to 32 and its length from 70 to 77. The cause is the redundant branch in item 2. The refactor is worthwhile but not blocking.
- **Intentional churn on this diff:** The churn rows on `proposed`, `choices`, `run` and `interactive`, plus the quickstart `tests` rows, come from edits to this one uncommitted diff. The `srv_is_explicit_…` verbosity growth (54→75) is the intentionally added SRV test assertions.
- **Artifact:** `interactive` at `marmish_native_keychain_check.py:31` is not a real finding. That file is unmodified and only imports the helper by name. Ripwire is attributing the change in `quickstart_check.py` to a symbol with the same name.
- **False positives:** All nine `dead-code` rows are `#[test]` functions that the test harness calls.

I'd commit as is, or do the item 2 refactor first if you want the complexity gate clean.

## Dispositions

No blocking findings. The extra parse branch implements distinct identity and
alias selector rules; retain the small local change rather than adding helpers
solely for a complexity score. Duplicate immutable alias lookup is bounded.
The actual candidate PTY run covers the dialog override. Python remains CI-only.
Network activation in the offline fixture is an expected property of the setup
path, verified by code inspection and owner-stop reports, not a network-capture
assertion; the report field must be interpreted with that limit. Full Rust and
live validation are recorded separately in the validation report.
