# Private Manual receiver actor design review

Claude Code Sonnet 5.5 independently reviewed the
[actor design](../phases/peer-candidate-manual-actor.md) in Auto mode on
2026-11-18. The design-only work is tracked in `marmish-dwt.4.12`.

| Review | Exact design | Session | Disposition |
| --- | --- | --- | --- |
| R1 | `afdd63001be920431103bfc9e754090d7f86d510` | `acc6789b-4737-4608-9dbe-d8f6d76f0110` | Requested changes |
| R2 | `2814a0e5f47c4dff91144cf02a04f75782791bb9` | `f705b6ea-340e-49a7-8f59-05d1560edbce` | Approved |
| R3 | `e450de00aa7df9a5b64d06556091e9af34b32a14` | `d075c5ef-b91c-4fa2-887b-cdb4827095e1` | Approved the exact follow-up |

R2 resolved R1's requirements for exact prerequisite revisions, runtime and
read signatures, synchronous revocation during native turns, Manual
policy mapping, one pre-attached quota, managed package provisioning and
crash recovery, a dual-clock maintenance bound with measurements, and
leaf-specific compilation boundaries. It also fixed the disconnect policy,
named the empty-view proof and stated the effect of a delayed startup
call. The reviewer checked replay and the distinction between local refusal
and process-fatal default dispatch loss.

R3 confirmed the corrected reference to the installed checkpoint aperture in
`peer_installed_offer_store.rs`. The implementation may use that file's
existing mount under `peer_consent_checkpoint` or a narrow re-export.
Its exact integrated mount remains part of the pre-implementation review.

Review used read-only source inspection. No Rust code, builds, network tests
or installed qualification were undertaken. Managed package creation and
attestation and the composed source and installed acceptance gates remain
required.
