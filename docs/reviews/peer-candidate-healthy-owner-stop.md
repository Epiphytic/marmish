# Registered healthy owner stop code review

The [owned-stop design](../phases/peer-candidate-owned-stop.md) was reviewed
at `9c9fe52d77b70b3aab1cf8e594a483a6bd1d1c83` in Claude Code Auto
session `ab3a6ad3-e23d-4bcc-a4dc-b2ddbb51b07d`.

Claude Code Sonnet in Auto mode requested changes to `0548fda` in
session `e34e78d5-940e-4a03-b7e3-fe6ff0d3f1a9`. The requested coverage
included actual engine drain and quiescence, an interrupted in-flight
turn, transfer refusal, lost observer, invalid owner evidence, and a
sticky cut during graceful preparation. The author added these cases.
An early local test incorrectly expected a joined engine to provide its
own quiescence evidence. The replacement tests separately prove a real
quiesced engine authorizes healthy cleanup and a joined unquiesced
engine permits physical completion with cleanup withheld.

Claude Code Sonnet in Auto mode approved intermediate code `e448333`
in session `0c65ab37-5750-459d-9957-940d422fa36c`. Its static review
used the exact commit. Its later runtime run saw the successor, and
its independent candidate build did not reach the checks. The final
review and author candidate checks use the fixed successor.

Claude Code Sonnet in Auto mode approved exact adapter source
`7701bb3d6c14f3dda7cfcc8870691e524fbc0362` in session
`611c2143-9d38-4965-9e1f-862aafb5bc5d`. The independent review checked
registered owner and worker custody, deadline and fatal races, cleanup
ordering, and the unchanged ordinary physical cut. It inspected the
archived executable and independently ran all ten healthy-stop cases.
All passed. No blocking findings remained.

The first deadline bounds one graceful prepare attempt. Later
observations can establish physical completion. The daemon stop owner
must independently prove listener completion and preserve the original
Command engine in its owner until this adapter acquires it. Stream
graceful-progress cancellation was reviewed for this Command-only
candidate. Streams remain outside the registered service scope.
Additional engine deadline replay coverage and a cut between the Journal
and Account closes were suggested. The tests cover retryable engine
progress and cuts during graceful preparation and runtime cleanup.
The daemon subprocess still needs its own real dispatcher displacement
and listener proof.

See the [validation record](../validation/peer-candidate-healthy-owner-stop.md)
for exact tests and boundaries.
