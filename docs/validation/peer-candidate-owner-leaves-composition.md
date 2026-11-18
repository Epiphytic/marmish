# Candidate storage and listener composition validation

Exact Linux AMD64 source `35b40cf`. Locked strict all-target Clippy for
Marmish Engine and Native passed in 37.51 s with the candidate cfg and
in 32.21 s without it.

| Exact binary | Results |
| --- | --- |
| Candidate `13333fcb8a0a234768c22f7bb3f5c72f9529eb7ee775e438b0b4606781b1aad6` | IPC 78 passed and two ignored. Queued restart proof passed in 11.45 s. Both private WSS Pending selectors passed in 90.50 s. All nine account custody subprocess cases passed in 38.33 s. |
| Stock `88c1ee8c6b54e2e0c13f1852dcd9d6a5574068d4c9db47be8f79fcd5b94e4cc9` | IPC 60 passed and two ignored, including refusal over the stock socket. |

The retained image was 2,416,640 bytes. Cold queued attestation took
321.46 ms and encrypted image re-digest took 42.78 ms. The virtual
relay was private loopback WSS, and the Pending fixture used its own
clock. The daemon still has no installed Pending actor. macOS and
Linux ARM64 remain outside this candidate build.
