# Candidate Command startup review

Claude Code Sonnet in Auto mode reviewed design R1 at `c0b7998` in
session `c9935da4-b77e-401e-9757-70bf9f42b256` and requested changes.
The revised design specifies lazy layout resolution at the existing
stock point, preserves source error mappings, and requires stop-only
custody after candidate startup errors. Claude approved exact R2
`4ad887a177a6f16c42915480b63a6013b24fe45d` in session
`7ee55a16-14f5-404d-9785-a789429439bf` after reading the affected source.

## Code

Claude Code Sonnet in Auto mode approved initial source `d9887b9` in
session `ebe5c8b8-8962-4712-a2ef-60c6ae023701` after inspecting the shared
adapter and tests. The final source strengthened the late-refusal case so
the spawned worker trips the same admission while the frontend checks it.

Claude Code Sonnet in Auto mode approved exact source
`a992a181315457f0f41e44924afdb3d361ff077b` in session
`3ff5a176-4759-46a3-b89a-d773008a2958`. The reviewer checked the final
diff against the approved design, verified the archived binary digest,
ran all five startup cases in 66.8 seconds, and repeated the late-refusal
case three times. There were no blocking findings. The reviewer noted
that the durable journal snapshot gives the primary cut evidence because
a tripped admission can short-circuit the verifier call, and that the
pre-spawn cut demonstrates no launcher entry. The stock layout case tests
the shared lazy resolution path. The test's five-second synchronization
bounds give explicit failures if the worker or frontend misses its barrier.
See [validation](../validation/peer-candidate-command-startup.md).

Claude Code Auto session `7e3ab257-dffd-443d-abb0-b08b35680856` approved
the exact composition `b1e1906` and reran the immutable candidate tests.
The changed source and its dependency entry points remained byte-identical
after the merge with the recipient actor. The review did not recompile the
composed tree. The registered daemon still does not invoke this adapter.
