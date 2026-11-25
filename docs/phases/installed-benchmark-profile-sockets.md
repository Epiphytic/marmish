# Installed benchmark profile socket correction

Publisher run [45706080756](https://github.com/Epiphytic/marmish/actions/runs/45706080756)
passed all 162 installed streaming and transfer cases. All three finite
benchmark jobs exhausted activation before command sampling. The daemon binds
its profile-derived socket under its child `TMPDIR`. The benchmark still
passes generated `/tmp/mb-*` paths to the launcher and then polls those paths.
The streaming runner corrected the same mismatch in `86b40cb`.

Resolve the server and client sockets with the same existing helper the passing
streaming runner uses. Resolve both after setup against the explicit child
`Environment`, reject collisions before spawning either daemon, and keep using
the client socket for authenticated command and readiness requests. Remove the
benchmark's temporary socket directory allocation and deletion. The daemon and
existing cleanup functions retain their current ownership and authority over
runtime and engine state. Preserve all sample counts, validation, timeouts,
cleanup requirements, and automatic publication gates.

Run the existing real-profile IPC regression and benchmark unit tests. Re-run
an installed same-platform benchmark with the corrected build and an explicit
relay. Keep local observations distinct from the final candidate. Publication
still requires a fresh source-bound candidate and the complete automatic gate.
