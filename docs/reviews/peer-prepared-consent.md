# Prepared-stage Consent proof review

Claude Code independently reviewed the [phase design](../phases/peer-prepared-consent.md)
and integrated Rust proof in Auto permission mode using Sonnet 5.5. The design
and code reviews examined the exact base `95ce7683d0bcc0ed8720011be5b187e1bbb6610d`
and its locked upstream MDK `9a29d1e46f11bc8e421046d5a1e20b52b59795df`.

| Review | Claude session | Integrated fingerprint | Verdict |
| --- | --- | --- | --- |
| Design R3 | `96068bcf-bf06-4608-9b13-e8aa5acc504f` | Phase document SHA-256 `adf197432ce32b133b7de7a6176789632cdd78e021515004ac7184164e72cdee` | Approved |
| Code R1 | `5fd6e23e-2fe6-497d-a1d0-4bfb3db8087d` | `23acc898a7baf3a29b6a1b1d8897693ea01fae6e`, tree `fb6ba7d608ae2d98fd527f2ae4e98b5c7719c97e` | Changes required |
| Code R2 | `0b18571a-b44e-4bf7-a5c7-6644e0be5bea` | `bef9e8cac87a0ed5740fbc9a7d62b7049002a53c`, tree `57aab7395dacf569d0584bc33eda4212c4f7d35e` | Approved |
| Standalone codec follow-up | `7432f641-6831-43ae-91ae-08328650fb38` | `00037033958021b0e5f1928c54917258ee2bd9b5`, tree `ebf4297bf3180c640a0bb1b3ba36a8855de4c15a` | Approved |

The final design required inspection of a bounded encrypted copy because
MDK opens databases read/write and checks migrations on open. The fixture
compares the original and copy digest and inode, removes the copy before
returning authority, and fails closed on inode changes, cleanup errors or
expired Consent. Recovery of an already committed frame can pass the Offer
expiry without signing again.

Code R1 found no logic defect and held approval on missing adversarial tests.
The follow-up tests exercise legacy claims, corrupt slot and stage references,
a real SQLCipher cursor-write fault and rollback, wrong-key or migrating
native images, sidecars and damaged data, and frame, carrier and package
mutation. The standalone cursor tests include the shared source directly.
Its capability placeholders are uninhabited, leaving actual native read
and ACK authority with the distinct library fixture.

Code R2 verified that the follow-up covered all five findings. The reviewer
passed seven focused library tests and 33 standalone cursor tests, with
the opt-in cases skipped. The reviewer suggested retaining the production
artifact scan alongside the benchmark results. The author repeated both
correctness suites and the artifact check on the final source. The fault
fixtures use disposable directories that are removed when their owners
drop.

The proof uses the local sink and a separate test-only schema variant. The
next phase can consume this evidence when designing exact Welcome custody and
activation. See [author validation](../validation/peer-prepared-consent.md)
for exact commands, non-test symbol inspection and local timing results.

Strict workspace Clippy exposed unused fixture codec helpers in the
standalone wire test. The follow-up exercised the capped signer, exact
sender and recipient transcript, and expiration in an existing test.
Claude verified the test-only diff and passing lint and codec logs.
