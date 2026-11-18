# Native Peer SelfRemove and cold recipient readback

Source `68c95cfcb336ac95a79a88ca417f7a44e34b7b6b` on
`0644345b9112c45cf0e235af4d37548d3d05e023`. The test used Linux, disposable
Nostr identities, upstream MDK `59b7ed3ff007494b25f8404c144c1d82809b3973`,
OpenMLS `6100df3fb1d8c3eeab53e349fcbd621f98c44fe2`, and a private SQLCipher
directory. The local transport adapter captured exact typed requests and
returned test acknowledgments without opening a relay connection.

The first native run with MDK's default empty registry reached the founder's
native SelfRemove schedule, then failed during commit construction with
`ProposalValidationError(UnsupportedProposalType)`. OpenMLS treats SelfRemove
(`0x000a`) as an explicit proposal capability and requires its presence in
remaining leaves. The fixture now calls the public
`SessionConfig::feature_registry` and installs the same required SelfRemove
capability as upstream's application registry. Both disposable accounts
advertise it before generating their signed leaves. No upstream patch was
required.

The corrected fixture sends one genuine recipient SelfRemove proposal. It
matches the missing-route refusal to the retained Sent event, verifies the
recipient's own MLS proposal, restages behind a closed adapter, and reopens
the same DELETE-journal SQLCipher file before a single exact typed ACK. The
ACK leaves the recipient in the roster. Founder native ingress schedules and
commits the proposal without a separate administrative remove. A stable
founder advances one epoch and loses exactly the retained recipient leaf.
The recipient processes the exact sealed signed commit and emits a voluntary
termination event. Independent native authority after SQLCipher reopen shows
the exact processed commit, successor epoch, founder-only roster, terminal
recipient state, and cleared LeaveRequest. Tests reject a wrong ACK shape,
a repeat publication, tampered commit digest, a wrong recipient store and
a wrong processed-ID argument.

Reproduce with `cargo test --offline --locked -j2 -p marmish-native --lib
peer_native_leave_auto_commit_and_cold_remove -- --ignored --nocapture`.
The bounded test is opt-in and uses no relay or installed runtime path. It
keeps the sealed commit in memory while the original native handles close
and reopen. A process crash, durable pre-ingress seal, terminal journal,
retirement integration and installed capability negotiation require their
own owner validation. The timing clock starts immediately before the
recipient Leave request and excludes fixture account/group setup. These
are local proof timings and do not estimate end-to-end WSS latency.

The exact opt-in test passed in 44.87 s, including setup. Capture plus MLS
verification took 360 ms from the decision point. Exact proposal
acknowledgment completed at 819 ms cumulative, and native removal at
3,435 ms cumulative. The recipient SQLCipher cold readback took 226 ms.
`cargo clippy --offline --locked -j2 -p marmish-native --all-targets -- -D warnings`
passed in 11.36 s on the warmed build target. Rustfmt and `git diff --check`
passed on the exact source. These commands ran on the exact commit above.

The same exact built unit harness passed both pre-existing opt-in leave
cases in 24.84 s with two test threads. They covered missing-route exact
event recovery and the native public runtime's exact-event re-drive.

The same two source paths were applied unchanged to the published
configuration-parser branch in `94f0e34`. The native proof passed in 46.36 s.
Capture, acknowledgement and removal took 341, 740 and 2,978 ms cumulative.
The independent reopen took 168 ms. Strict native all-target Clippy passed
in 32.11 s on this integrated tree.
