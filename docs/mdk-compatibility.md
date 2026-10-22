# MDK compatibility evidence

Evidence date: 2026-10-22. Source inspection and offline checks are supplemented by the bounded live phase-2a exchange below. This is not a verified production command integration.

The source checkout is not clean: its staged diff covers 24 files (+35/-1656), with SHA-256 `d4ab779c3b548b9e7a1a1d2ed3735d17c45d1cdd2715a15b449e6304c0122f83` for `git diff --cached --binary`; the unstaged diff was empty. Evidence below is reverified from committed blobs using `git show 37c248a:path`, rather than attributing staged work to the commit. No checkout files were changed.

The inspected source is [liamhelmer-bel/mdk](https://github.com/liamhelmer-bel/mdk), revision `37c248a11239e108afc9bd4ee62b8f60542012e3`, available read-only at `/home/openclaw/repos/mdk-fork`. The installed executable `/home/openclaw/.local/bin/wn` reports `wn 0.13.0`; its SHA-256 is `c1f5a130549a142070b477303f1cf1e50153ab33b61815b5c7b4f681546747e5`. This observation is not a provenance attestation. Subsequent phase-2a preparation observed `/home/openclaw/.local/bin/wnd --help` and SHA-256 `1324c70d389fb49f37ae8f7f6ab2a16840b138a0c438d7aef10bb6f9eee20a03`; it has no version flag. Neither hash establishes source provenance. The pair is a candidate for the explicitly authorized synthetic smoke, not an approved production dependency.

## Source findings

| Surface | Inspected evidence | Consequence |
| --- | --- | --- |
| Daemon | `crates/cli/README.md`, `src/daemon/protocol.rs:247` | `wn` fronts `wnd`; documented CLI is preferable to private CLI-shaped socket RPC |
| Explicit defaults | `crates/cli/src/daemon/runtime_host.rs:370` | Supply both discovery and default-account relay sets; this alone does not enforce egress |
| Peer routing | `crates/marmot-app/src/directory/member_key_packages.rs:335` | Peer relay records can introduce additional destinations |
| Relay safety | `crates/marmot-app/src/relay_plane/safety.rs:47` | Host safety checks are not an operator-selected relay allowlist; loopback needs a development opt-in; private-network support needs verification |
| Public defaults | `crates/marmot-app/src/runtime/mod.rs:7733` | Built-in discovery indexers exist and cannot be accepted as fallback |
| Application messages | `crates/cli/src/args.rs:998` | `messages send-event` CONTENT is argv; sensitive payload exposure and OS argument limits block use as the production data plane without a safer interface |
| Relay flags | `crates/cli/src/args.rs:1423` | Comma-delimited list flags; reject commas in URLs to avoid adding implicit relays |
| Secret store | `crates/cli/src/args.rs:39` | Headless operation needs global `--secret-store file`; filesystem protection remains unverified |
| Subscriptions | `crates/cli/src/daemon/subscriptions.rs:676` | Default snapshot 50, capped at 200; typed NDJSON is not an application durable cursor |
| Request bounds | `crates/cli/src/daemon/mod.rs:60` | 1 MiB daemon request ceiling, 256 local connections and 64 subscriptions; Marmish needs much smaller application frames |
| Retention | `crates/cli/README.md:291,400` | Founding `--retention 1d` and explicit `messages sweep-expired`; daemon does not schedule sweeps; unread/skew/scan cases may defer cleanup |
| Devices | `docs/explorations-of-multi-device.md:1` | Exploratory/non-normative; production enrollment, attribution, revocation and history sync unverified |

Paths in this table are relative to the inspected MDK source. The detailed [phase 0 plan](phases/phase-0.md) records proposed documented CLI shapes. Synthetic adapter fixtures are contract tests, not captured live MDK responses.

## Relay boundary decision

Select a native, enforced MDK relay allowlist covering every connection path as the preferred integration boundary. It must constrain peer-advertised routes as well as explicit defaults and cover imported identities, discovery, key packages, invitations, group traffic and retries. A reviewed isolated MDK checkout can implement this prerequisite separately; this phase does not change the shared MDK checkout. Additional operating-system egress isolation is useful defence in depth, but address-only firewalling does not by itself prove an exact URL/relay identity policy across DNS, TLS, proxies or redirects.

No such boundary is verified at the inspected revision. Therefore phase 0 starts no network-capable MDK process. The production live adapter remains disabled pending Beads `marmish-315.9`. The user explicitly authorized bounded synthetic tests with disposable identities on selected official public relays; [phase 2a](phases/phase-2.md) may proceed without claiming this production boundary. The original blanket live-probe prohibition was an overinterpretation. The acceptance test must inject a decoy relay outside the configured set in authenticated peer routing records and demonstrate zero attempted out-of-set connections. Passing local relay happy paths does not satisfy this requirement.

| Boundary candidate | DNS and TLS identity | Redirects, proxies and subprocesses | Decision |
| --- | --- | --- | --- |
| Native MDK strict URL allowlist | Normalize and authorize each requested relay before DNS/TLS; preserve TLS hostname verification, enforce every reconnect | Reject unconfigured redirects/peer routes; prevent ambient proxy/analytics egress; audit all daemon dial paths | Preferred, selected but unverified; implement/review in isolated MDK worktree |
| Network namespace plus nftables and IP-literal relays | Address constraints are strong for fixed IPs, but certificates/relay URL identity still need validation; dynamic DNS requires controlled updates | Includes subprocesses in namespace; proxy destinations and shared hosting can bypass URL-level intent unless additionally constrained | Optional defence in depth; not selected as sole URL policy |
| seccomp/connect-filter sandbox | Syscall address filtering does not establish DNS names or WSS application identity by itself | Must inherit across descendants and constrain all socket/address families, proxies and alternative syscalls | Higher platform/implementation complexity; not selected alone |

Acceptance for the selected boundary includes a clean build/artifact pin, explicit all-route policy, proxy/analytics handling, and packet/dial instrumentation while malicious peer records advertise decoys. No phase-0 fixture proves these properties.

## Retention and catch up

Default application retention is 86,400 seconds. Marmish must schedule explicit sweeps and report failures/deferred cleanup. Content retention must not delete required MLS group/epoch state; that separation still needs live validation. Local expiry never guarantees erasure at relays or other devices.

Subscriptions require application sequence numbers, per-stream cursors, deduplication, paginated history catch-up and gap reporting. A message publication result is not proof the remote application accepted or acted on a request. Commands need their own execute-by token; a one-day message lifetime is not a one-day launch authorization.

## Capability matrix

| Capability | Status |
| --- | --- |
| Installed `wn --version` and binary hash | Observed as above |
| Matching verified `wn`/`wnd` provenance and capability pin | Unverified source provenance; observed pair passed the limited synthetic smoke |
| Configuration and offline subprocess contract | Phase-0 offline adapter and phase-2a synthetic smoke approved; see separate review artifacts |
| Strict configured-relay egress | Missing prerequisite; live gate closed |
| Real two-identity room/message exchange | Passed limited phase-2a synthetic smoke; see recorded evidence below |
| Authenticated event/device schema | Account sender/room/message schema observed in phase 2a; per-device schema unverified |
| Scheduled expiry preserving MLS state | Unrun; live integration required |
| Noninteractive remote command execution | Later phase |
| PTY/tmux sessions | Later phase |
| Multi-device enrollment/revocation/conformance | Unverified upstream prerequisite, Beads `marmish-315.10` |

Dependency fetches, if needed, are permitted build setup rather than relay traffic. Verification uses `--offline --locked` after the lockfile/cache are prepared; exact checks and outcomes are recorded with phase review evidence.

## Reproducing the bounded observation

```sh
/home/openclaw/.local/bin/wn --version
sha256sum /home/openclaw/.local/bin/wn
```

These commands do not validate live networking, source provenance, or a compatible daemon pair. Production room/message operations remain gated. The separately reviewed, explicitly opted-in phase-2a smoke may exchange fixed non-sensitive probes using disposable identities and explicitly supplied relay URLs.

## Phase 0 offline verification

On 2026-10-22, the integrated workspace passed:

```sh
unshare -rn cargo test --offline --locked --workspace
cargo clippy --offline --locked --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
cargo build --offline --locked --workspace
cargo run --offline --locked -p marmish-mdk --example probe -- /home/openclaw/.local/bin/wn
```

The network-isolated test run passed 7 configuration/framing tests and 9 adapter tests; `unshare -rn` is supported on this host. The probe reported `wn 0.13.0` and explicitly labeled this an unverified binary observation. At phase 0, daemon, room, authentication, retention and multi-device checks were unrun; phase-2a observations below supersede that historical status for its limited scope. The adapter tests use synthetic fixtures rather than captured authenticated MDK responses.

Initial integrated and independent Claude runs caught a fixture fork/exec race (`ETXTBSY`); serializing fixture setup/execution fixes that test race. Process cleanup also retains the unreaped child identity until group signalling finishes. A subprocess deliberately escaping its group using `setsid` is outside the probe's containment guarantee. The version probe is for an explicitly trusted local executable.

The fixture fix passed 50 consecutive adapter-suite runs, followed by 30 complete workspace-suite runs under four bounded CPU-load processes. The load processes were killed and reaped after verification. The long successful-output fixture uses a 10-second test deadline; the production version probe retains its 2-second deadline.

Claude Sonnet 5.5 (`claude-sonnet-5-5`) approved the completed phase in `docs/reviews/phase-0-code-final.json`. The reviewer independently reran offline and network-isolated tests, formatting, Clippy, the real version-only probe, and 10 loaded adapter-suite repetitions without failure. Remaining phase-2 validations include generated CLI argument validation, hostname length policy, and executable-mode checks.

## Phase 2a explicit public-relay preparation

The user explicitly permits bounded synthetic tests on selected official public relays; this corrects the earlier blanket live-test restriction. [The official Transponder configuration](https://raw.githubusercontent.com/marmot-protocol/transponder/master/config/default.toml) lists `wss://relay.eu.whitenoise.chat` and `wss://relay.us.whitenoise.chat`. Direct DNS resolution and HTTPS requests with `Accept: application/nostr+json` succeeded for both on 2026-10-22. They returned EU/US Internet Privacy Foundation strfry relay documents advertising NIPs 1, 2, 4, 9, 11, 28, 40, 45, 70 and 77. These probes establish reachability only, not a Marmot round trip, an SRV contract, or retention enforcement.

No official DNS SRV service-name/resolution contract has been verified. The smoke uses explicit WSS URLs, canonicalized with a root slash, rather than inventing one.

The [phase 2 detailed design](phases/phase-2.md) separates synthetic compatibility testing from production integration. Claude Sonnet 5.5 (`claude-sonnet-5-5`) approved the then-current design in `reviews/phase-2a-plan-final.json`, after findings recorded in `reviews/phase-2a-plan-initial.json`. Source-backed corrections to the published-count and received-direction fields followed that approval; the completed-code review checks the corrected implementation. The approval covers a standalone developer smoke with disposable identities and fixed non-sensitive messages. It does not open the Rust adapter's production gate.

## Phase 2a live result

On 2026-10-22 the explicitly opted-in smoke passed against both selected White Noise URLs. The [sanitized report](reviews/phase-2a-live.json) records the executable hashes, public identity/group/message IDs, exact assertions and limitations. Two fresh local signing identities advertised exactly the configured relay lists, created and accepted a two-member room, observed 86,400-second application retention, and received matching kind-30100 probes in both directions. Receipt checks matched remote sender, room, message ID, incoming direction and exact synthetic payload. The CLI spells that direction `received`; send-event `published` is a positive integer count. Private state and owned child process groups were cleaned up successfully.

Reproduction requires the explicit trusted executable artifacts with the recorded digests; it creates new disposable public identities and relay events each time:

```sh
python3 tools/mdk_smoke.py --live \
  --wn /home/openclaw/.local/bin/wn \
  --wnd /home/openclaw/.local/bin/wnd \
  --wn-sha256 c1f5a130549a142070b477303f1cf1e50153ab33b61815b5c7b4f681546747e5 \
  --wnd-sha256 1324c70d389fb49f37ae8f7f6ab2a16840b138a0c438d7aef10bb6f9eee20a03 \
  --relay wss://relay.eu.whitenoise.chat \
  --relay wss://relay.us.whitenoise.chat --timeout 180
```

This is one successful bounded exchange, not a load, restart, expiry or production command test. MDK supplies sender authentication; the harness does not independently verify MLS. Reported relay lists do not prove enforced routing. Binary source-build provenance remains unknown, public KeyPackage/relay-list events may remain, and observing retention configuration does not certify eventual deletion. SRV remains unverified. The production adapter is unchanged and gated.

The implementation initially passed 17 offline Python tests; review follow-up expanded coverage to 23 passing tests with syntax compilation; the unchanged Rust workspace passed 67 regression tests with `cargo test --offline --locked --workspace`. A scoped ripwire quality-delta check could not compare against a baseline/HEAD for this new untracked tool, so no clean quality-delta claim is made. Claude Sonnet 5.5 approved the completed code in `reviews/phase-2a-code-final.json`; it independently ran the 23-test suite three times and syntax checks without generating public traffic.

The initial completed-code review (`reviews/phase-2a-code-initial.json`) found no code blocker, requested clearer separation of historical phase-0 approval from pending phase-2a approval, and suggested stronger offline coverage. The documentation now distinguishes those scopes; the added tests passed and final approval is recorded in `reviews/phase-2a-code-final.json`.

Review follow-up added a positive npub/checksum vector, exact command construction checks, main cleanup/error/interruption restoration coverage, identity schema mutations, and descendant liveness checks that tolerate dead zombies. The runner itself is unchanged from the successful live test; its SHA-256 is recorded in `reviews/phase-2a-live-provenance.json`. No repeat public exchange was needed.
