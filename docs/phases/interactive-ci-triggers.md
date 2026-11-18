# Interactive CI triggers

The interactive branch changes native qualification helpers under `tools/`.
The legacy v0.1 installer workflow treats that entire directory as an
automatic trigger and schedules six hosted jobs. The Node coordinator
workflow also schedules a hosted job because the branch changes its tools.

Keep the legacy workflow available by explicit manual dispatch. Limit the
installed coordinator workflow to the existing path-filtered trusted
`master` push and use the Linux X64 `bel-ci` runner in the existing
`epiphytic-ci` group, with the same repository and ref guard as the native
core job. Run the existing locked Node dependency install and tests on that
runner. The runner needs Node and npm on its normal tool path. PR code never
runs on a persistent runner. Keep checkout credentials disabled and the
read-only Contents permission. Add the core wrapper's exact path to the
native core trigger. Its test selection and test commands remain fixed.

Check the workflow changes by parsing the YAML and comparing runner labels,
events, guards, permission scopes and command lists. Run the coordinator
suite locally. A missing runner leaves Actions unverified. The historical
manual legacy matrix and release dispatch workflows keep their own scope.
