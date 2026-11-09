# Legacy managed test environment correction

Bead: `marmish-d93.6`. Base: `fa119c6e876f3bed779945d0a3daee4275437058`.
This phase repairs optional legacy test tooling. It changes no native runtime,
release qualification requirement, namespace boundary or workflow execution.

The confidential diagnostic [validation](../validation/legacy-confidential-trace.md)
identified exactly two missing-PyYAML import errors on both Linux platforms.
macOS provisioned Rust 1.97.1 successfully, but the fixture's first isolated
`rustc -vV` invoked rustup seeking an unavailable stable toolchain. The exact
wrapper path and bytes were not retained; a sibling-installation mismatch is
a conditional explanation requiring hosted validation.

## Approved design before implementation

Claude Opus 5.5, session `34a9953e-e4cb-4ad1-aa90-01e7f0915da9`,
reviewed the original design and requested preservation of the self-hosted
bootstrapped direct compiler. Original design SHA-256:
`2252b78eabdcf372f4e556d57151789882cd15cd99688b1aa7aa90ba1e1544b6`.
Amendment SHA-256:
`44d9bf687cbc0b9490a251ebcc23b9c55285326197ddca5359faedabd7152500`.
The follow-up design verdict was **APPROVE**, evidence SHA-256:
`6205162b50be327f5e0341e6dfbb800857c6722bf7f1e55874171761c56b2f02`.
Implementation starts after recording this approval. Completed code needs
independent review before publication.

1. Preserve direct toolchains without a rustup sibling, even with unrelated
   rustup on PATH. `native-validation.yml` installs such a toolchain with
   `marmish_ci.py`; its compiler selection must remain unchanged.
2. Inspect the unresolved absolute PATH rustc directory for a rustup sibling.
   A present sibling must resolve strictly to an executable regular file;
   malformed/broken/nonexecutable siblings refuse instead of falling back.
   Prefer this sibling for the existing fixed `rustup which --toolchain 1.97.1
   rustc` lookup. Otherwise retain existing PATH-rustup proxy detection.
   A selected lookup cannot fall back to direct or stable and cannot install
   anything. Retain sanitized host lookup env, cwd `/`, 10-second bound,
   `RUSTUP_AUTO_INSTALL=0`, strict absolute/name/file/executable checks and
   final proxy rejection against both known installations. Use existing
   `rustup_explicit_1.97.1` provenance. All three tools retain existing exact
   version/host/hash/fence checks. The sibling guard applies to selected
   rustc; existing PATH proxy guards and exact probes remain the backstop
   for cargo/rustdoc, without adding provenance plumbing.
3. Remove PyYAML from the two static workflow contract modules. Preserve
   their seven original contract methods with stdlib-only, indentation-
   anchored job/step/block checks, exact inventories and scoped permissions,
   credentials, artifacts, actions, commands, triggers and eligibility guards.
   Treat run scalar bodies as opaque. Comments or unrelated/nested fields
   cannot satisfy formerly structural assertions. Do not implement YAML,
   add a dependency, skip imports or weaken trust boundaries.
4. Add in-memory mutations using per-path test-owned text sources. Inner
   suites run only the original contract class (exactly three or four
   methods, zero skips); restore patched sources unconditionally. Cover
   moved/commented guards, nested always, extra artifacts, changed permission/
   credentials/env, unpinned action, token, interpolation, artifact pattern,
   concurrency and source/package/event guards. Benign comments still pass.
5. Resolver mocks cover distinct installations/sibling preference, direct
   compatibility, absent PATH rustup, bad sibling/lookup/output/proxy cases,
   unchanged sanitized environment and no auto-install. Record actual RED
   before helper changes and focused GREEN afterward. Run no actual Cargo,
   compiler probes, namespaces or full integration suite locally.

Use existing pinned managed Python with `-I -B` for the explicit mock inventory,
from repository and tools cwd with namespace flag absent/literal `1`. Record
actual counts; new mutations change discovery totals. Run diff and quality
checks, then actual Claude auto completed-code review. Hosted Linux/macOS
operation remains pending until a later parent-authorized workflow run.

## Completed increment

The [completed review](../reviews/legacy-managed-environment.md) records
Claude's final **APPROVE**, after fixing scalar-continuation, whole-step token
and literal-body margin findings. The seven original workflow contract methods
remain, with scoped in-memory negative controls. This is deliberately an exact
layout contract: unsupported quoted/folded/chomping/multiline scalar forms,
continuations (including deeper content after intervening comments), and
literal bodies lacking the full fixed margin refuse. It does not parse YAML.

The focused managed-Python inventory passed 102 tests with zero skips in each
of four repository/tools cwd and namespace-flag absent/`1` configurations;
Claude independently repeated those checks. Meaningful import, resolver and
mutation RED/GREEN evidence is recorded in the review. No real compiler,
Cargo, namespaces or hosted tests ran locally. Diff checks passed; reviewed
quality findings remain explicitly recorded without suppressions. Actual fresh
Linux full-suite and macOS compiler/cache validation remain pending.
