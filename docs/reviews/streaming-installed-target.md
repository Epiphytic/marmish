# Installed streaming public destination reviews

Beads `marmish-8ka.5.7`. Baseline source
`86b40cb8f36f7dea311d232770b8aa5351bd0405`; scope is the
[bounded target correction](../phases/streaming-installed-target.md).

## Independent design review

Actual Claude Code invocation: `claude -p --permission-mode auto --model
claude-sonnet-5-5 --output-format json --max-turns 18`. Session
`57a467cd-cf01-41c9-ad28-103ce1c1953f`, elapsed 94.314 seconds,
22 recorded turns. Verdict **APPROVE**, no blocking findings.
The reviewer independently inspected product target parsing, all public
stream/copy/rsync/cancel/edge argv boundaries, internal raw account bindings,
finite callers and the pinned Nostr dependency contract. No source edits,
Cargo tests or live workloads ran in this design review.

The reviewer confirmed the public rawhex mismatch, complete boundary inventory,
strict fixed-category adapter, unchanged internal hexadecimal identities and
unchanged product rawhex refusal. Useful suggestions adopted for implementation:
exercise targets extracted from actual builders through the product parser,
propagate fallible builders without unwrap, retain literal/non-UTF8 copy paths,
check rsync's UTF-8 remote suffix unchanged, and observe the negative CLI probe
without a large stdin write. `PublicKey::from_hex` in the pinned dependency
validates encoding rather than curve membership; the helper's strict lowercase
64-hex predicate is the relevant input check. A local npub success remains Linux
same-host evidence only.

## Independent integrated code review

Actual Claude Code invocation used the same auto/model/output options with
`--max-turns 26`. Session `0f8d4522-2302-4342-b1bf-7fa629e489f5`, elapsed
116.141 seconds, 15 recorded turns. Verdict **APPROVE**,
no blocking findings. The reviewer inspected every changed Rust file, new docs
and sanitized evidence against the approved design. All fourteen public argv
insertions are covered; raw journal, account, nonce and IPC fields are unchanged.
No product, schema, public installer, timeout or workflow budget changed.
Reviewed ten-file source-set SHA256 is
`ae94597f379a896b03c1082b23906337b752cb6400134b51f0838b22991a1c18`
(canonical sorted path-to-file-SHA256 map); root verified those source bytes
unchanged after review.

Reviewer-run checks:

- Ten new contract tests passed, covering generated npub round trips through
  the actual product parser, strict fixed-category invalid input refusal,
  family argv positions, rsync suffix preservation and a non-UTF8 copy operand.
- Complete `installed_check::` tests passed serially: 163 passed, zero failed,
  22.57 seconds. One parallel run also passed: 163 passed, zero failed,
  5.68 seconds.
- Strict installed-feature all-target Clippy, `cargo fmt --check` and
  `git diff --check` passed. The reviewer changed no source content, ran no
  live relay workload, and made no commit or push.

Author validation had ten focused passes and a complete qualifier run with
162 passes and one `ETXTBSY` failure while executing a freshly copied printf
fixture in the unmodified benchmark test. A single bounded serial rerun passed
all 163 tests in 22.40 seconds. Strict Clippy passed in 9.04 seconds, along with
formatting and diff checks. The reviewer considered the fixture failure
consistent with a transient fork/exec race outside the changed paths, without
claiming the race fixed. Fresh hosted full parallel suites remain mandatory;
no ignored test or qualification waiver was added.

Quality inspection found no new complexity, nesting or parameter regression.
The nonzero static quality delta reported short-horizon churn in an existing
rsync function, test-discovery dead-code artifacts and token-similar small argv
or assertion fragments. Exact new production clones were consolidated into
shared installed argv fragments before review. Remaining fragments did not
justify a broader refactor in this correction.

Nonblocking review findings were addressed by updating the phase status and
recording this integrated review. The reviewer read but did not independently
rerun hosted jobs, local live probes or author quality measurements; those
retain their separately stated provenance. Local installed Linux byte/argument,
journal-binding, idle-runner and clean-stop evidence is preserved in
[the failure and reproduction record](../validation/streaming-installed-target-failure.json).
It does not qualify other hosts. Fresh three-platform native/package gates and
all eight serial installed cohorts are still required before qualification.
