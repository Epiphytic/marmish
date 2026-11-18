# Installed Peer control v2 design reviews

The detailed plan is [Installed Peer control v2 and status-room coordinates](../phases/peer-installed-control-v2.md).
It uses the approved [Installed Peer heartbeat first](../phases/peer-installed-heartbeat-first.md)
design at `3db4a36` and the lab base `7ace82e`.

Claude Code CLI in Auto permission mode, model `claude-opus-5-5`, reviewed
both revisions independently with local read access on 2026-11-18.

| Revision | Exact commit | Claude session | Verdict |
| --- | --- | --- | --- |
| R1 | `4670322b16dc9d30ee46934856fb47ff6a82ac0e` | `038332b1-9628-4f9a-8f70-c5fb012dcb77` | Request changes |
| R2 | `2767d11c43dcdacdbc8120b89884a9a9b6c1a320` | `eec502df-0202-4ff9-be54-f1318c72db17` | Approve |

R1 identified two blockers. Existing v1 `Decoded` values can be constructed
from checkpoint fields, so decoded content needed an opaque v2 type with its
own canonical parser. Authentically re-signed binding tests needed typed wire
bodies in canonical field order so that a syntax failure could not mask a
missing binding check. R2 confirmed both fixes and independently recomputed
the fixed status room vector. The approved design file SHA-256 is
`d99317304608560b081eb9a94e4b843787ddfdff9c3a54e19c3c3ead598e8866`.

The review checked both callers of the early Delivery classifier, the static
custody conflict path and the Prepared Consent transcript reconstruction.
A Delivery with the wrong version now fails at classification and enters
the existing invalid path before any new receiver effect. Valid v1 message
processing and receipt bytes retain their existing format. The approved plan
leaves installed checkpoint retention, native MLS sender and current epoch
verification, and status authorization for the later owner.
