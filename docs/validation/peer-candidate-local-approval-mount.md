# Installed candidate-local approval validation

The private Linux AMD64 candidate and stock were built as optimized native
packages and installed into separate fresh prefixes with the shipped Rust
installer. Neither package was published. The candidate used source
`dca025b35a71f17be860a914890995d89e033070` and manifest SHA-256
`9754147e2275c5c8bc70343dbd3bf98181d3f5d7d7ffb4e02b31f4ea63d48794`.
The stock Command package used source
`06f7de75b4590074102a338965a9f829c678ea15` and manifest SHA-256
`ec441683117f72a33b45b6c48a118213a1cef9e5fd1edcbb9a46aa8339460102`.
The manifest's payload digests and the installer receipt were checked
through the normal installation and executable-layout paths.

The corrected test harness from `bcc2a7a` used the original candidate's
commit stamp in its test executable so the executable-layout check matches
the installed package. The installed binaries were the original optimized
archives. Source after the candidate package adds test changes and an
extraction of the existing Manual lifecycle check, reviewed as behavior
preserving. The stock and candidate libtests from `44c4c25` both include the
fresh-epoch expectation changes.

The stock custody and CLI suite passed 12 tests. The candidate approval
suite passed 16 tests with its installed opt-in case deselected. The
corrected candidate CLI and actual same-UID approval socket selection
passed 12 tests, including two shared command-line checks. These selections
overlap and should be counted individually. The new retired-port case
exercised the port refusal and the lifecycle recheck without a relay.

The first installed run received success from the CLI and an immediate
retry before timing out on an invalid harness expectation. The private
reader's only production projection is a verified Pending entry. After
the approval invalidates that proof, it answers Unavailable. The corrected
case checks that response independently from continued registered route
health and the offline SQLCipher decision record.

A second attempt stopped at native layout verification before startup. Its
new test executable initially carried a newer source stamp than the
installed candidate manifest. Rebuilding the test executable with the
candidate package stamp allowed the original installation checks to run.
That run recorded the decision, passed the invalidated read projection
and found the exact Activating row and both monotonic floors. Its keeper
assertion had searched fixed Active-stage slots. The candidate's joined
file is the checked provisional file. Harness revision `f557090` uses
the stored device and inode with the same binding and checked reopen
path as the product.

## Installed result

The corrected installed case passed in 54.36 s on Linux AMD64. It used
disposable identities, a private registered WSS Peer relay, a separate
private ordinary Command relay and the installed native applications.
An authenticated founder drove Offer, Consent and Delivery. The recipient
projected the verified Pending invitation, refused the wrong generation,
and ran the matching installed `marmish servers approve` command. The
command emitted exactly `recorded local approval` and exited zero.
An immediate exact retry succeeded. List and Show withheld the invalidated
Pending proof while the registered route was still live. The test checked
that the Peer relay saw exactly the three allowed control publications.

When the private Peer route closed, ordinary Command still returned the
exact expected bytes. The cold SQLCipher read found the Activating row,
checked both persisted wall floors and reopened the original keeper by
recorded device and inode. After daemon restart the original selection
again returned success, with no second decision or observable Peer egress.
The test checked the same decision timestamp and keeper, and monotonic
floors, after the restart. The separately installed stock daemon read both
ordinary command results from the profile.

| Measurement | Elapsed |
| --- | ---: |
| First offer through verified Pending | 22,643 ms |
| Installed CLI process launch through the approval reply | 799 ms |
| Immediate typed Unix socket retry | 289 ms |
| Cold candidate restart through exact retry and projection check | 1,785 ms |
| First ordinary command | 4,489 ms |
| Ordinary command after Peer route close | 3,872 ms |

These are single-run loopback measurements. The CLI timing includes process
startup and foreground waiting. The test stops at the local decision that
records Activating. Receipt, heartbeat and group publication remain separate
qualification work.

## Internal decision measurement

The follow-up used optimized candidate CLI, daemon and worker from
`b5c0671`, the stock package from `06f7de7`, and an unoptimized
qualification driver and libtest for the same candidate stamp. The libtest
included test-only changes through `b24e71d`. The first run stopped before
Consent with the diagnostic `manual_package / clock`. The same installed
package and test reran successfully in 62.84 s.

| Measurement | Elapsed |
| --- | ---: |
| First Offer through verified Pending | 22,843 ms |
| Installed CLI process launch through first approval reply | 1,071 ms |
| Daemon decision on the first approval | 559.935 ms |
| Immediate exact retry at the Unix socket | 544 ms |
| Daemon decision on the immediate exact retry | 465.560 ms |
| Cold restart through exact retry and projection check | 4,242 ms |

The daemon samples only its approval decision. IPC queueing, listener and
CLI process time are outside that interval. Both internal samples came from
the first daemon, with `--debug` logging only the duration and event schema.
The cold restart measurement remains an end-to-end sample. The positive
case also passed with the integrated test code through `824623d` in
66.50 s. It measured 572.528 ms inside the daemon for the decision,
439.125 ms for the exact retry, and 1,124 ms through the first CLI reply.
These two passing samples leave the earlier clock refusal unexplained
and are insufficient to estimate a latency distribution.

## Installed schema isolation

The composed test passed in 34.27 s with the same candidate and qualification
driver and the test code through `824623d`. It created two fresh profiles
through the installed daemon, one with Manual v2 and one with Approval v3.
Each original mode established authenticated subscriptions before stopping.
The opposite mode then lost its Peer worker locally before dialing the
registered relay, with the ordinary local socket still active. The test
checked that neither refusal acknowledged an approval or changed any cursor
bytes. The matching mode reopened the same cursor and again established
authenticated subscriptions. The relay observed no control publications.

The initial installed schema run attributed an EOF after the Peer worker
closed to an explicit Unavailable reply. The client instead returned
Uncertain. The corrected test accepts either refusal while separately
checking the worker exit, zero new relay traffic and unchanged cursor.

## Committed local reply lost

The composed test passed in 65.61 s using the same optimized candidate and
stock binaries with the unoptimized test harness through `824623d`. The
installed CLI submitted the legitimate approval through a one-connection
same-UID test socket. The test verified the daemon's successful reply for the
exact selection before dropping the connection to the CLI. The CLI returned
exit 1, empty stdout and `marmish: operation_uncertain` in 962 ms.

The first cold SQLCipher check found the committed Activating row and original
keeper before any retry. After restart the installed CLI repeated the exact
request on the daemon socket and reported success in 790 ms. The second cold
check found the same decision timestamp and keeper and both wall floors
remained monotonic. The Peer relay still counted exactly three enrollment
control publications. Two ordinary commands returned the exact bytes and
the stock daemon reopened both records. No product fault-injection option
was added.

The combined candidate source passed locked all-target Clippy with warnings
denied on Linux AMD64. The three installed scenarios are opt-in.

## A later signed invitation during authenticated history

The installed test passed in 62.05 s on Linux AMD64. It used the optimized
candidate at `b5c0671` and the unoptimized qualification driver and libtest
with candidate test source `ba92201`. The first installed daemon verified a
Pending invitation and refused approval with an unrelated generation. After
that daemon stopped, the same founder signed a later Offer for the recipient
with a fresh Offer ID.

The private relay wrote that exact Offer and EOSE on the restarted
recipient's authenticated bounded history subscription. The receiver
reported a cursor refusal and closed only its Peer worker and route.
The approval of the original selection received no success and ordinary
Command still returned the exact expected bytes. The relay saw exactly
four control publications, including the second Offer.

After shutdown, a read-only SQLCipher check found the same Pending
selector, generation and retained MLS file, including its identity and
digest. The original Offer marker remained the sole accepted Offer and
there was no deferred row for the later carrier. The same custody
comparison passed in an earlier 63.64 s run before adding the bounded
signing-clock wait. The first attempt reached the cursor refusal, then
stopped on a test assertion that expected Unavailable from the approval
client. The revised test allows EOF uncertainty on the closed worker
and checks refusal independently of the client error category.

Current custody retains at most one invitation per founder. A second Offer
aborts that recovery turn while preserving the original Pending. A future
replacement policy would need its own durable transition and validation.
Locked all-target Clippy with warnings denied passed for both the stock and
candidate source on Linux AMD64.
