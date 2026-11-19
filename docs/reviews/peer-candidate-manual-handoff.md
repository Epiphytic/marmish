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
