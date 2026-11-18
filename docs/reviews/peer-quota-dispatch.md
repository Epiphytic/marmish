# Peer quota dispatcher reviews

The [phase design](../phases/peer-quota-dispatch.md) was approved by Claude
Code Sonnet 5.5 in Auto mode. The final design review is session
`419850f0-61f6-43aa-bc36-170680dfe4b4`, on design SHA-256
`906e4a1defd1a6a984d56fc85fdd892be7a1ac30b4dbc38b6efb9891caa73132`.
The approved plan keeps the dispatcher test-only until complete Linux and
macOS qualification and a separate activation review.

## Linux AMD64 proof

Claude Code Sonnet 5.5 in Auto mode approved the exact implementation and
tests in `e65bed1597afadf2fffe6a0c76a4adfe67fc0bf6`, tree
`889ceb4d045ab32da4100227acf7fd77fed6c900`, in session
`0499b0ba-cfda-4fde-b164-7495836b1214`. The reviewer read the diff and
independently reran the frozen test executable, which passed all seven quota
harness entries in about 18 seconds. See the
[author's validation report](../validation/peer-quota-dispatch.md) for the
build identity, commands and platform limits.

The review found no blocking Linux proof defects. It identified two low-severity
hardening items for subsequent qualification. Reconciliation can change
OS errno after a failed managed write while preserving the SQLite return code.
The reservation guard can handle a poisoned reservation mutex more explicitly
during Drop. The review also noted the process-global single profile,
sticky quarantine on validation failure, and the limits of the two-thread
test. It proves that an ordinary thread keeps its stock behavior while a
managed scope exists on another thread. It schedules those operations
sequentially while the scope is held.

The verdict applies to the test-only Linux proof. Darwin/APFS, true concurrent
I/O, installed Command and Terminal with the dispatcher, and production
configuration and activation remain outside this approval.

## Diagnostic and unwind follow-up

Claude Code Sonnet 5.5 in Auto mode approved the exact three-file follow-up
`5e93c1537a520ea5abc5d1ae3d3d234e91a79ecf` (tree
`842412ade0ac596e9b22e3a9dd3768f82dddda7b`) in session
`a0204f98-85de-48ac-bcb9-4e8df6ed2a90`. The reviewer verified the prebuilt
Linux AMD64 binary hash and independently reran all seven quota harness
entries, including the short-write and poisoned-lock cases.

The follow-up addresses both previous hardening items. The reviewer confirmed
that bundled Unix SQLite reads thread errno directly and that the guard on a
poisoned lock clears only the matching active claim, quarantines that stage,
and leaves later turns blocked. The stock open and delete errno paths were
reviewed in source. The write path has a regression test. The poison test
uses intentional lock poisoning, and wrapper-origin late open failures
retain their existing handling. The Linux-only test scope is unchanged.
