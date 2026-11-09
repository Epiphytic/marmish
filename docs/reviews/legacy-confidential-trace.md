# Confidential legacy diagnostic review

Source baseline: `6eef3c1ace4971883dfa065fa0b04c8f5197be16`.
The [phase design](../phases/legacy-confidential-trace.md) was written before
implementation. Independent Claude Code (`--permission-mode auto`), model
`claude-opus-5-5`, session `4056b28b-6368-4a24-b159-52ce24ea71f4`, inspected
the actual source and reviewed all four new implementation files explicitly.

Design review first required disclosure that the new contract module joins
normal discovery, then required explicit workflow-level concurrency for the
three-platform matrix. Both findings were resolved before implementation.
Final design verdict: **APPROVE**, response SHA256
`6ab15599ffeaf0234c7d8ede460077c625fd8a062b5b65d72dcc6c85bc320373`.

Completed-code review found that the existing `*.pem` ignore rule would omit
the public certificate. The certificate was explicitly force-added without
weakening secret-file exclusions; its indexed blob was independently verified.
Final completed-code verdict: **APPROVE for integration**, response SHA256
`78f3cee1853d612de4d13076871926d1ed911e3570d48327577e740ed7df9e14`.
Initial completed-code response SHA256:
`f373b8cef04c089b9f8e25f1eef2f0491e4da5589f31138527d18d0bc4502d21`.

The author ran exactly nine mock-only cases, zero skips, with managed Python
3.13.13 from repository root, tools, and disposable home, with the parent
namespace flag absent and set to1. All six configurations passed. The reviewer
independently repeated the nine cases under both flag modes, and checked shell
syntax with both sh and bash. A separate actual local OpenSSL3.0.13 roundtrip
preserved2097152 synthetic bytes; ciphertext2097725 bytes stayed below2162688.
No full suite, namespace workload, native build or hosted diagnostic was run.
Hosted macOS CMS availability remains unproven. Expected comparable Linux suite
count is996 with11 skips, not an observed success. A new-module failure,
count/skip mismatch or nonreproduction cannot explain the historical failure.

Quality-delta exited2. The reviewer accepted the small duplicated fixture setup
and tiny status tests rather than introducing a shared framework. Dynamic
unittest discovery accounts for static dead-code findings. The new workflow's
job name collides with an existing `diagnostic` symbol, producing a misleading
preexisting verbosity attribution. Keyword-only mock parameters fit the test
matrix. No metric acknowledgements or suppressions were added.

Before dispatch, the integrator must verify all four implementation blobs,
including workflow and test, in the actual published commit on both mirrors.
The certificate must be tracked, with blob
`a27aba634c18d8a9d7ea72100009699129247216`, file SHA256
`aff49d908e50a4a3c7f6955224c9f6e0adcb43dea935df991d8110fd23f01a78`.
Its SHA256 certificate fingerprint is
`22:ED:D9:57:0F:30:E3:B0:70:D7:98:9A:D3:F5:44:9C:55:EB:04:A2:F9:2D:F6:8D:CE:DD:A5:2B:DC:17:EF:AA`;
it expires2026-11-16 at14:52:49UTC. Only this public certificate is published;
the ephemeral private key stays outside Git. Publication, operational review
and one-shot dispatch remain the integrator's separate duties. Encrypted
evidence acquisition does not establish legacy suite or native release success.
