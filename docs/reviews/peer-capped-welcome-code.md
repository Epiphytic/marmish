# Capped recipient Welcome code review

Claude Code `claude-opus-5-5` independently reviewed exact source
`3b2a00f29895e2d28882289acd985c7aada2c579` against `383c071` in
`--permission-mode auto`. Session `1471623c-fffe-4128-aed1-03a071fe7961`
returned **Approve** with no blocking findings. The reviewer inspected all
changed source, tests, and approved design evidence. The reviewer examined the
exact author validation logs and performed static review. The [author's tests
and timings](../validation/peer-capped-welcome.md) ran separately.

Claude checked that the held row and verified Delivery bind the same stage,
carrier, signer, route, and profile lease. The one-use prepared and JoinStarted
proofs bind the original effect. The checks cover the native samples before
open and ingest, the aggregate VFS budget, synchronous one-poll close, and
readback with a separately accounted encrypted copy. Claude confirmed that the
installed binaries remain outside the fixture scope.

The proof assumes the private profile directory and exclusive profile lease
used by the existing native stage owner. A separate process running as the same
UID and modifying files in place falls outside that owner model. Snapshot
quarantine fences all coordinated fixture operations for the rest of the process,
including Generate and Reopen. Recovery evidence here means a fresh native open
on a new encrypted copy during the same fixture process.

The successor phase owns the capped-only Pending CAS and actual v2 WSS proof.
Its focused cases should include a mismatched stage generation, another valid
package binding, and a stop between native open and ingest. The proof here
uses a valid v1 transcript, tests cancellation after durable JoinStarted and
before native open, and observes the specific journal refusal through VFS
counters. The design and authorization evidence is recorded separately.
