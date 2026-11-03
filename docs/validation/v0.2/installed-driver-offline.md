# Installed matrix driver: offline validation

The build-only `installed-checks` feature is frozen for independent integrated code review. This evidence records offline checks; it does not claim installed, network, cross-platform or release acceptance.

- `cargo test -p marmish-native --features installed-checks installed_check --lib`: **16 passed**, 0 failed; 5.01 seconds test execution. Includes exact flat twelve-record/nine-cell rollup, missing/extra/failed records, pinned tooling schema, strict records, byte vectors, cleanup facts/fence, and owned subprocess byte/exit/output/deadline/environment checks.
- `cargo clippy -p marmish-native --features installed-checks --all-targets -- -D warnings`: **passed**, 3.59 seconds.
- Targeted `rustfmt --edition 2024 --config skip_children=true --check` over the nine driver/fixture files: **passed**.

The rollup emits JSON and Markdown with one observed sample per directed pair and no percentile estimates. Command wall time includes the installed CLI process lifetime; throughput uses validated stdout plus stderr bytes and excludes stdin. Actual measurements remain pending hosted execution.

The cleanup authority design and independent approval are [the sealed authority design](../../phases/v0.2-installed-cleanup-authority.md) and [its actual review](../../reviews/v0.2-installed-cleanup-design-r3.md). Feature-gated helper integration requires a live retained owner process, peer credentials, unchanged final fence, all server ledger rows and the original tree receipt. Driver code approval is not implied by that design approval.

## Frozen source SHA-256

```text
4b890e3f98a0a18365eb1727b0794a4e51d61eb867c8cdb81b695c60a03f882a  crates/marmish-native/src/installed_check.rs
f6b0390dab2ddac92c39898bff8bad4936ad1fe9372ab1c29eccb59f8929c54c  crates/marmish-native/src/installed_records.rs
9def03ae31c6781e7b084ff4d0e37d45abbc169ec70b067f6c84be7cc9399b82  crates/marmish-native/src/installed_process.rs
8f313a043e1806875dd9f794be66c34744ab7adf9ceab2ab592aa099e163d414  crates/marmish-native/src/installed_prepare.rs
5d2f474732c3578d1f4f5440f40964fb55886b5023a08aba6860af52060b314d  crates/marmish-native/src/installed_fixture.rs
a244eb6f3beb0252eda03fa300129d7cb01b2ca2351d3a0cb1d26435e41db314  crates/marmish-native/src/installed_cleanup.rs
4d4112d8394a8ee55f7a4c438462000f850b1841abb2e611059e48f006205957  crates/marmish-native/src/installed_matrix.rs
ce4f84e048c5ac5f99df7167110e43f2cbb29650e4fbb3554f118b70c72d1a8f  crates/marmish-native/src/bin/marmish-installed-check.rs
0e974700e1623a744c3cbac46f769921d20122e862dade363716bbbc3736d796  crates/marmish-native/src/bin/marmish-installed-fixture.rs
```


## Integrated review r2 corrections

D1 now derives work and protected cleanup from one server-launch monotonic origin:
work stops360 seconds before the fixed uptime and cleanup stops30 seconds before
it; the uptime itself is at most1800 seconds and fits before the original Context
expiry. Budget arithmetic is re-evaluated after offline setup and refuses too
little remaining time.

D2 runs installed `marmish setup --update` with all three npub grant/trust options,
checks its closed report and exact resulting principal sets, and changes only the
fixture allow_exec leaf, cwd and limits afterwards. The offline regression runs
the actual native setup implementation for both roles and verifies those fields
survive the read-modify-write; installed hosted execution remains pending.

Additional hardening rehashes the copied fixture against its tooling pin, checks
the same pin in server-ready/rollup validation, refuses any pre-existing engine
path using no-follow metadata, and kills/reaps an owned process group on stop
timeout while aborting/joining its drains and retaining a failed result. A real
TERM-ignoring owned shell exercises that timeout path. Strict feature Clippy and
targeted formatting pass. Ripwire quality-delta exits2 against the shared dirty
HEAD with124 structural findings across several increments (including bare-name
cross-language/feature reachability dead-code and existing clone matches); this
is recorded as a limitation, not a clean quality verdict. Clippy verifies the
feature-enabled code has no compiler dead-code warnings.

## Integrated review r3 correction

Expected grant/trust keys are sorted before exact comparison with installed setup
output; duplicate expected keys are refused. The actual-setup regression now
deliberately reverses peer input, verifies setup canonical ordering differs from
that input, accepts the exact set and refuses both missing and extra principals.
The16 focused tests and strict feature all-target Clippy pass after this fix.

## Client startup limit correction

Benchmark design inspection discovered that max_commands on the matrix client
would be rejected by daemon startup. Matrix now configures server4/1800 and
client uptime1800 without a count. The actual setup regression passes BOTH final
configs through the daemon's apply_limit_overrides(Default) path and verifies
their effective count/uptime values. All16 focused checks, strict feature
all-target Clippy and targeted formatting pass after the correction.
