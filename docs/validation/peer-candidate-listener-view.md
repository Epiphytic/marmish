# Candidate Pending listener qualification

Exact Linux AMD64 source `925d5a9f2ae57adb91d3f38721951280cfe4b81f`.
Locked all-target strict Clippy passed in 29.54 s for the explicit
candidate and 39.96 s for stock. The candidate executable reported
78 passed and two ignored in the IPC suite. The stock executable
reported 60 passed and two ignored. Their SHA-256 values, respectively,
were `d6c038debe3cdcc2df7a2f0c8083f2695db4ad8f1ab79fc50266af053a76b860`
and `41575fe00d4dd4e5f87a552077b36b76ff29d3f5497cae95bfa1901a11b09b9a`.

Eight focused same-user socket tests cover exact listener pairing, the
two readiness edges, a fifth concurrent read, fatal refusal, stopped
view refusal, dropping an unregistered offer, aborting the served
future, and revocation before drain cancels a still-admitted task.
Stock sends the raw candidate request over a real socket and receives
`invalid_request`, with ordinary Status still available. The exact
build script refuses candidate selection for Linux ARM64, macOS ARM64
and Linux AMD64 with `installed-checks`. This verifies compile-time
selection and local socket behavior. Installed actor Pending/Empty
readback still requires qualification.
