# Installed streaming public destination correction

Implemented after independent Claude design approval; integrated code review
also approved. See [review evidence](../reviews/streaming-installed-target.md). Baseline
`86b40cb8f36f7dea311d232770b8aa5351bd0405`. This is a bounded correction to the
feature-gated installed qualification driver. Product CLI, transport, daemon,
installer pins, artifact schemas and workflow deadlines remain unchanged.

## Failure and contract

Installed stream matrix run
[42356736438](https://github.com/Epiphytic/marmish/actions/runs/42356736438)
reached server-ready on Linux AMD64, Linux ARM64 and macOS ARM64, then all nine
duplex cells failed rapidly without receipts. Cleanup records report completed
stream cleanup and incomplete server-stop proof. Those observations do not
prove that commands ran, that transport failed, or that an owned child leaked.

`installed_streaming_cases::duplex` passes `ServerReady.server_account` directly
to the installed `marmish stream` command. The validated artifact account is 64
lowercase hexadecimal characters. `command_target::account` deliberately
rejects raw hexadecimal destinations; public commands accept npubs or configured
aliases. This mismatch is sufficient to prevent every duplex Open before any
remote command executes. An independent actual installed-candidate reproduction confirmed rawhex exits
255 with `invalid_remote_identity`; the equivalent npub duplex completed in
21.997 seconds, exit 7, preserving 98,304 stdin/stdout bytes and 8,192 stderr
bytes plus special argv. Both owned daemons then stopped cleanly with their
actual stop reports true. See [the sanitized reproduction](../validation/streaming-installed-target-failure.json).
This establishes the local boundary defect and one Linux installed npub path,
not cross-platform qualification. Other streaming qualifier families repeat it. Their
internal IPC, room bindings, hashes and evidence correctly use raw hexadecimal
and must keep doing so.

The existing input subprocess helper can obscure this refusal: if the child
exits before consuming the 96 KiB fixture, its stdin write fails and the helper
returns `installed_child_input` after aborting diagnostic drains. This design
does not expand into subprocess error reporting. The regression must capture
bounded child output without a large stdin write so the target refusal itself
is observable. Never publish raw subprocess stderr, private paths or identities
as a replacement for the existing sanitized failure categories.

## Shared boundary and exact implementation scope

Add one installed-only, pure helper to `installed_records.rs`:

```rust
pub(super) fn public_destination(account: &str) -> crate::Result<String>;
```

Require exactly 64 lowercase hexadecimal characters using the existing
`records::hexadecimal(account, 64)` predicate. Parse `nostr::key::PublicKey`,
then use the existing `nostr::nips::nip19::ToBech32` implementation to return a
canonical npub. Every refusal maps to the fixed local category
`Error::Invalid("installed_public_destination")`; never interpolate the input.
No fallback to the original string, alias lookup, DNS, configuration mutation,
key generation, environment mutation or new dependency is allowed. Conversion
happens immediately before inserting a public CLI destination into argv.

Update these existing boundaries, preserving all other arguments and literal
`OsString` operands:

| File / function | Public destination boundary |
| --- | --- |
| `installed_streaming_cases.rs` / `duplex` | `stream` target |
| `installed_transfer_cases.rs` / `fixture_checkpoint` | fixture `stream` target |
| `installed_transfer_cases.rs` / `rsync` | identity before `:` in rsync remote operand |
| `installed_transfer_cases.rs` / `copy` | `put` / `get` target after `--` |
| `installed_copy_interruption_cases.rs` / `arguments` | all initial, Busy and Resume copy attempts |
| `installed_edge_preopen.rs` / `run` | paused-server `stream` target |
| `installed_edge_quota.rs` / `args`, `cancel` | held/refused streams and explicit cancel |
| `installed_edge_admission.rs` / `one` | rsh admission and missing/denied stream attempts |
| `installed_edge_signals.rs` / stream and cancel construction | signal/detach start and explicit cancel |
| `installed_edge_rsync.rs` / `run_case` | identity before `:` in rsync remote operand |
| `installed_edge_copy.rs` / `attempt_args` | all ordinary and Resume copy attempts |

Make `arguments`, quota `args`, and copy `attempt_args` return
`Result<Vec<OsString>>` and propagate failures through existing Result-returning
callers. Their input remains the raw account; each builder performs the shared
conversion itself. Existing pure argv tests must use valid raw account fixtures
and assert the resulting npub, rather than silently accepting arbitrary target
strings. For rsync, convert only the identity component before formatting its
existing remote operand; preserve path bytes and quoting policy. Do not convert
journal comparisons, checkpoint principals, cancellation stream IDs, nonce or
Open digests, account artifact fields, or direct IPC remote fields.

`installed_copy_detach.rs` and `installed_copy_interrupt.rs` receive public argv
from interruption `arguments`; they have no independent account destination to
convert. Verify all three interruption builder callers propagate errors.

## Separate finite-cohort audit

Existing finite callers in `installed_matrix.rs`, `installed_benchmark.rs`,
`installed_policy.rs`, `installed_recovery.rs`, `installed_autoexit.rs` and
`installed_version_lifecycle.rs` already use `PublicKey::from_hex(...).to_bech32()`
at remote public execution boundaries. Setup grants similarly convert to npub.
Leave these working conversions in place; consolidating them is unnecessary
scope. Lifecycle direct IPC and command journal operations intentionally retain
raw hexadecimal accounts. The independent finite-cohort socket issue remains
tracked separately as `marmish-8ka.5.6`; this target correction does not qualify
or repair finite cohort readiness.

## Validation before publication

1. Shared helper tests: a valid generated public account round-trips through
   npub and the product account parser to exactly the original 32 bytes;
   deterministic canonical output; empty, short, long, uppercase, nonhex,
   npub-as-input, private-key and traversal-like values return only the fixed
   category. Use disposable fixture keys and publish no key material.
2. Exercise the actual product `command_target::parse` contract with private
   test configuration: the original raw account still fails with
   `invalid_remote_identity`; its converted npub resolves to the expected
   account. This test must fail against the old raw CLI construction. Do not
   add product rawhex support to satisfy it.
3. Builder tests verify exact positional target extraction for stream, copy,
   rsync `npub:path`, cancel, admission and interruption/Resume families. Where
   construction is inline, extract a small pure argv builder only when needed
   for meaningful regression coverage; keep family-specific flags local. Check
   special filenames, literal `--` boundaries, compression/Resume flags and
   remote path suffixes are unchanged. Include a non-UTF8 operand on Unix.
4. Establish a real installed public CLI reproduction with the same owned
   disposable profiles and authenticated daemon socket. Before correction,
   capture the rawhex `invalid_remote_identity` exit without claiming remote
   execution. After correction, run the installed binary with the converted
   npub against the owned remote and require actual fixture output, exact
   byte/hash checks, terminal journal binding and physical cleanup. Root owns
   relay scheduling; do not overlap another live qualification. A parser test
   alone or successful publication is not this proof.
5. Run focused installed-feature unit tests, complete native installed-checks
   tests, strict all-target installed-checks Clippy, formatting and exact diff
   audit. Obtain actual Claude Code auto integrated code review of all affected
   builders, new files and tests; fix useful findings before publication.
6. Publish reviewed source to both remotes, build a fresh immutable candidate,
   and require all native/package gates on Linux AMD64, Linux ARM64 and macOS
   ARM64. Then run all eight installed cohorts serially: stream-matrix,
   rsync-matrix, copy-matrix, edge-stream, edge-rsync, edge-copy,
   copy-interrupt-put and copy-interrupt-get. Retain all nine client/server
   cells per matrix and existing signal/cancel/quota/Resume evidence. Failed
   or absent receipts, incomplete stop proofs and timeout failures remain FAIL.

The first local installed success only establishes that particular public CLI
path on that host. The candidate gates and eight cohorts are required for the
full installed streaming qualification. Preserve the failed run and subsequent
sanitized evidence separately; do not rewrite its result or infer platform
success from a source review. No release or deployment is authorized here.
