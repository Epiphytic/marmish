# Installed fixture compiler validation

Local Linux AMD64 validation used a fresh digest-pinned Rust 1.97.1 bootstrap,
then a disposable compiler-only state. The source installation contained no
symlinks or hardlinks and copied successfully into private state. This validates
the compiler boundary; it is not installed Marmot transport or cross-platform
interoperability evidence.

The actual locked public-registry fetch and offline dependency warmup completed
in 6.388 seconds. Three incremental native fixture builds, one for each fixed
client-role label, then passed exact binary stdin/stdout/stderr and receipt
validation. Each exited 7. The Linux-x64 label returned 65,813 stdout bytes;
Linux-arm64 and macOS-arm64 labels each returned 65,815 stdout bytes; all returned
8,192 stderr bytes. These are three label configurations compiled and executed
on Linux AMD64, not executions on those other architectures.

The complete successful workload took 8.040 seconds. Final cleanup completed in
0.214 seconds with all seven checks true: frontend, MDK, workers and caffeinate
absent; engine cleanup ready; uninstall complete; root removed.

Two preceding attempts found a helper Path/string bug (fixed), and a conservative
process-scan failure on a newly unreadable `/proc` entry. Both failed disposable
states also completed all seven cleanup checks. No authentication or permission
check was weakened to obtain the successful result.

Focused subprocess tests prove that a compiler group accepts unregistered forked
children, times out with owned group cleanup, rejects a marked setsid escape,
and refuses global cleanup without signalling that escaped identity or deleting
state. The test captures and explicitly cleans its own escaped identity.
Fixture tests cover byte preservation, exclusive receipts, rejected input and
argv including argv0, invalid or absent role/path configuration, credential and
wrapper environment scrubbing, Cargo config poisoning, managed-toolchain links
and changed bytes, preparation reserves, and private directory modes. A native
Rust unit test verifies that the fixed 120-second alarm is armed. Rejected
executions intentionally leave an empty exclusive receipt; only a complete,
validated receipt is completion evidence.

The actual independent compiler design review is recorded in
[the review evidence](../reviews/installed-nine-pair-compiler-design.json).
Native macOS/ARM64 toolchain-copy compatibility and the full installed nine-pair
transport workflow remain separate acceptance requirements.
