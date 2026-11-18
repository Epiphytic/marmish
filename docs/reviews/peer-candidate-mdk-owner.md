# Upstream MDK account custody review

The design review used Claude Sonnet 5.5 in Claude Code Auto mode.
Session `07924203-24d4-4bae-a80f-f1c67ea052ff` approved the fixture
design at `1ebf685`. The qualification inventory was clarified by
`eaa4ccb`.

Session `2304bdb1-8103-4bf6-9e67-d48413a15b07` approved exact fixture
source `f6d6d84`. It independently ran all nine subprocess cases in a
cleared environment and inspected the locked MDK and rusqlite close
contracts. The observed release marker can also be set when close returns
an error. In the fixture, successful close is checked separately against
main-file descriptor release, sidecar cleanup and cold reopen.

The shared-handle path was inspected using the locked MDK sources.
The sibling scan in `check_unexplained_commits` can take the idle lock
of another already registered connection to check its in-memory autocommit
state. It does not open a new connection. The current connection uses
its own checked lock for the `data_version` probe. The fixture keeps an
exact strong account root until exit after a cut and verifies ordinary
access refuses after `begin_close`. Physical task shutdown, a live
held guard and actual upstream close failure remain outside this fixture.
