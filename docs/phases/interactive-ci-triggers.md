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
runner. The runner needs Node 22 and npm on its normal tool path plus registry access
for the locked install. Removing both PR triggers and the legacy push
trigger covers PR creation and merge. The persistent runner accepts trusted
master pushes. Keep checkout credentials disabled and the
read-only Contents permission. Add the core wrapper's exact path to the
native core trigger. Its test selection and test commands remain fixed.

Update `tools/v02-actions/coordinator-policy.test.mjs` in the same change.
Assert the dispatch-only legacy trigger, filtered trusted coordinator push,
self-hosted group, labels and guard, and native core wrapper path. Change
the routing matrix to reflect the legacy workflow's lack of automatic
routing. Keep the existing read-only checkout and locked command assertions.

Parse the workflows to cross-check runner labels, events, guards, permission
scopes and command lists. Run the coordinator suite locally with Node 22.
Check Node availability on the self-hosted runner before relying on its
coordinator job. A missing runner leaves Actions unverified. The historical
manual legacy matrix and release dispatch workflows keep their own scope.
