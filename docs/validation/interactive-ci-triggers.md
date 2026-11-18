# Interactive CI trigger validation

At `ef88d78` the controller parsed and compared the three edited YAML
workflows with their previous versions. The six legacy installer jobs
are now manual. The coordinator keeps its locked install and test
commands on the trusted self-hosted `epiphytic-ci` Linux X64 runner.
The native core job still has 35 selectors. Only its path trigger
changed, to include the script those selectors call.

With Node 22.22.0 and npm 10.9.4, `npm ci --offline --ignore-scripts` and
`npm test` succeeded in `tools/v02-actions`. All 257 tests passed in
1.794 s. Git whitespace validation passed. The repository runner API
reported zero visible runners at the time of local validation. No
GitHub Actions job was dispatched.
