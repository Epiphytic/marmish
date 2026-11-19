# Private Manual listener handoff design review

Claude Code Sonnet 5.5 in Auto reviewed the exact design against the native
candidate, startup, read listener, view, and qualification sources. No build
or live tests ran as part of the design review.

The first review of `3671071`, session `1e23669c-eddd-4e8d-8261-bccb67e5a9c5`,
required a separate positive Pending proof after the service runner exists.
It also required direct Reader evidence independent of the readiness and
Fatal fences. The revision specifies ownership of the exact Producer by the
caught future, explicit registered Fatal provenance, a process-local view
stamp, and unavailable behavior during the inert handoff.

Claude approved exact `4ece6dc`, session `b10e8ad2-2bef-4718-9dca-83f8723fed60`.
Implementation should test a foreign Fatal, update the owned actor call
sites, and retain a Reader clone independently of the live registration
while the exact reap is parked. The positive installed test remains
conditional on approval of the complete service, including its projection
refresh cadence and route-loss behavior.

## Integrated implementation

Claude Code Sonnet 5.5 in Auto reviewed the integrated Rust and tests.
R1 `e13f227c-d5fb-40d7-9b23-424c3fb34d7f` requested direct Reader
admissions inside the caught port future and a test of actual
`Recipient::start` pairing. It also identified obsolete dead-code waivers.
R2 `e01177cc-e170-4381-ab75-32cc52897f22` found a race in
`cbf867a` between stop and receipt of the open
exchange's EOSE. The revision waits for the direct post-open admission
and moves private-type tests into the actual daemon module.

Static R3 `d7ebd284-d7ae-4da7-b310-f83a83fba4df` approved those
corrections at `7943903`. Integrated Auto session
`3039e479-b47c-4279-b12c-296a7bf6f9e9` independently approved exact
`7943903e2753e6e62be9d28a9734069b456d8696`. It ran all three private
argument/layout tests, the foreign Fatal refusal, and the actual paired
Manual and Passive listener cases in 4.69 s and 4.83 s respectively. It
reviewed exact Producer custody before raw route reap, fallback behavior,
and the test-only visibility. The author separately ran the packaged
controller and lifecycle checks in the [validation report](../validation/peer-candidate-manual-handoff.md).
