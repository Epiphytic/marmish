# Copy edge harness correction review

Scope: corrective installed qualification harness, parent source `2f98802d25e2a6ec9ebd995adfef3504169175b7`. No product protocol, release tag or installer pin changes.

## Independent design review

Claude Code, model `claude-sonnet-5-5`, `--permission-mode auto`.

Initial review: NOT APPROVED; session `e5ca99ae-d5f8-43eb-8a59-8b36f3ef6404`, 93.698 s, 14 turns, raw review JSON SHA256 `42f69bef1de6ca048630a7696dfe55ac065bf8bdb72e38d3efbb7f6b1347d03e`. Reviewer read actual source and identified two blocking design gaps: the filesystem regression must pass the identical parent through the actual constructor; diagnostic spans must include setup/postchecks and tests must not overclaim production result parity.

Revised review: APPROVE; session `d6f4c6d3-70f6-4bda-bd46-101c63c7e06b`, 14.508 s, 3 turns, raw JSON SHA256 `39bf19e42e3dd86d7e1369fc0277a5064ab0679ef0e062e46f83ac436817f651`. Approved phase document SHA256 `05e9c93f8e9b4fb2183d95ae105e24d466dbbf0b1db2eda3919892f45d9c8da2`. The reviewer checked the amendments against its initial findings; it did not repeat source inspection or run tests in round two. Both blockers closed; existing lossy error mapping is explicitly preserved and disclosed.

Implementation authority is the user's direct ongoing release-testing request and repository phased-build authorization. This is not a shared-queue task, so the generic queue human-approval reminder does not create another approval gate here.

Beads remains unavailable under the previously documented schema compatibility blocker; no database migration or bypass was performed. Review evidence is recorded here pending safe task-tracker recovery. Integrated code review and release qualification are still pending.

## Local validation before integrated review

- Actual filesystem mutation regression: temporarily changing both new constructor basenames back to `edge-copy-{nonce}` made `mdk_adapter::installed_check::edge_copy::tests::shared_parent_edge_roots_are_private_and_disjoint` execute once and FAIL with `AlreadyExists`, exit 101. Restored corrected bytes before further checks. An initial exact filter omitted `mdk_adapter::` and ran zero tests; it is not counted as evidence.
- Corrected installed-check module suite: 245 PASS, zero failed/ignored, 633 filtered, 9.03 s. This includes the shared-parent regression, exclusive file/symlink refusal tests and observer contracts.
- Scoped Rust 2024 formatting: PASS. No public relay or product runtime changes in this correction.
- Ripwire quality delta remains nonzero (exit 2); no baseline or acknowledgement suppression. Initial redundant copy-only observer bodies were removed in favor of existing data-driven contracts. Final report contains 29 rows, 18 gating: short-horizon churn on intentionally touched harness functions; verbosity from added bounded markers, explicit outer spans and finite test data; existing truncation-test boilerplate similarity; static test-entrypoint dead-code misses; and cross-file same-name JSON/Rust symbol attribution (`candidate`, `artifacts`) despite those product/old evidence files being unchanged. The two larger case wrappers preserve original checks/order rather than introduce a generic execution framework. These remaining findings require independent disposition, not a claim of a clean quality gate.
- Ripwire change-context bundle: exit 0, capped to 5000 tokens; broad name-based reach is structural signal, not a precise Rust linkage proof. Source scope is four installed-check harness files, compiled behind the installed-checks feature. Required runtime qualification is a fresh three-platform candidate and all eight original cohorts, not reuse of 90 previous observations.

Strict installed-feature all-target Clippy (`-D warnings`, locked/offline) PASS, 13.72 s; log SHA256 `db41d902d60d85b837547ce6b2a21772dbfacf1676098b05add1ce31a76d07fd`.

## Integrated review, first pass

Claude Code AUTO, `claude-sonnet-5-5`: NOT APPROVED, session `542406d2-f58c-4105-9500-db957ae92bce`, 129.072 s, 17 turns, raw JSON SHA256 `645a8556c6ef46ee4883ff20af03f02ce060a98978a91f12e3180548b5d15673`. Reviewer compared every production wrapper to the parent, verified unchanged order/error mapping/deadlines/cleanup/predicates, full postchecks and under-cap trace counts; independently reran all 245 installed tests PASS and checked all eight private evidence-input file hashes without reading identities. It found a blocking overlapping-span defect: a later source-refusal trace can emit Abandoned during an ordinary earlier failure. Amendment requires a shared source-preparation family span and sequential per-case traces. Publication remains blocked until correction and independent re-review. Reviewer found no independent quality risk beyond that defect in the inspected delta; the nonzero structural gate remains disclosed.

Narrow span-amendment design: Claude AUTO APPROVE, session `87b0a29d-98c9-46f9-bcc2-65ad8a74a6b0`, 56.688 s, 8 turns, raw JSON SHA256 `ec741d5275557afa1ffccf920d1610553753fcb7d6812edadf13122f06b87d43`. Actual source/Trace inspection, no tests; nonblocking clarifications incorporated before edits: 22 case plus two family labels, shared direction mapping, Baseline starts individual source spans, and actual filesystem ordering checks with no remote invocation claim.

Amended helper and sequential trace placement: installed module suite 246 PASS, zero failed/ignored, 633 filtered, 7.05 s. Actual helper test covers both directions with success, alias creation failure and later directory creation failure; it inspects real filesystem effects and collecting-sink events. Individual live invocation early return is verified by source review, not claimed exercised by that setup test. Latest unsuppressed quality delta exit 2: 32 rows/19 gating, SHA256 `c849118da685948d527d9cefd9340ed8d4fa72cd98a6f2fba456b8fed6161ded`; same disclosed structural categories, plus the new helper/test symbols.

## Integrated re-review approval

Claude Code AUTO, `claude-sonnet-5-5`: APPROVE, session `8c4c157c-9db9-4d45-9a01-595833597d54`, 98.370 s, 13 turns, raw JSON SHA256 `00a3112e0ead4fd33ed3cee2986c88c29054c85e98d747ccca7740eb822f517f`. Reviewer inspected the actual amended working source against the parent and independently ran all 246 installed module tests PASS. The overlapping-span blocker is closed: shared setup finishes its family trace, then individual source-case traces are created inside the sequential loop and finish before early return. Operations/order/error mapping/deadlines/cleanup/predicates remain unchanged; existing lossy category limits and source-review-only loop coverage are disclosed. The reviewer independently triaged the unsuppressed 32-row/19-gating quality report and found no additional blocking masking, API-surface or clone risk. This approval permits source publication and fresh qualification, not a release-qualified claim.

Final amended phase SHA256 `bd7aac2285c1f4dc01d91ac1942cc858040c6ef9b06581008da95c4b7f46f4f6`; the earlier design hash above records the initial approved document, not this amended revision. Final strict command was `CARGO_BUILD_JOBS=2 CARGO_TARGET_DIR=<shared-build-cache> cargo clippy --locked --offline -p marmish-native --features installed-checks --all-targets -- -D warnings`: PASS, 10.22 s, SHA256 `c674f63ea5fe15aea6f98750a71b47eaa8baeeece5bd8bbce0568f2e2d664e0f`. Root final 246-test log SHA256 `111e0caa7a2f533d96ce52dcdfe58c9e82c46f760c44863a6d7b442971e19b67`. Reviewer did not repeat Clippy/fmt or mutation RED; those rely on saved root logs. Index will be refreshed to include the approved amended source, excluding the Beads export. No new public version, tag or installer pin is changed.
