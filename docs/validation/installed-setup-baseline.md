# Installed setup measurements

One local Linux AMD64 observation using the actual install script and published
`0.1.0-experimental.2` archives, with a disposable HOME and prefix.

| Operation | Time | Samples | Result |
| --- | ---: | ---: | --- |
| Resolve and validate all three release archives/manifests | 8.928 s | 1 | passed |
| Execute real Linux AMD64 install script | 4.317 s | 1 | passed |

Static `marmish --doctor` checks and actual installer uninstall passed. Temporary
state was removed. These are setup measurements: **no Marmot command latency or
transfer throughput was measured in this observation**, and static checks do not
validate runtime confinement. No percentiles or cross-platform comparison can be
inferred from a single local sample.

The application source was `f609b6ccdc9302aeec661a2ae3692d3ef68b86fa`.
The installer SHA256 and verified per-platform asset identities are recorded in
[the machine-readable evidence](installed-setup-baseline.json). The separate
installed nine-pair workflow measures cold status, command wall time, first output
and verified payload goodput, and publishes failed cells without invented values.
