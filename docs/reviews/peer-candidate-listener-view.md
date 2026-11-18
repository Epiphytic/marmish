# Candidate Pending listener review

Claude Sonnet 5.5 in Claude Code Auto mode approved design `64e86a5`
in session `cc32b2b0-a100-4208-96ef-e37be0c4a104`. Code review
`5237e9a7-9ca8-4ea9-90c5-d66641262c13` found no production defect in
`c67c609` and requested direct shutdown and stock-refusal checks.
Supplement `925d5a9` adds those assertions without changing production
code. Review `2ad0d240-2291-4656-9c0b-fa559ce0a12f` approved it
against the design. It independently reran the candidate listener suite
with one and four threads and built the exact stock source in an
isolated target to verify that its actual socket refuses the candidate
request, leaves the owner queue untouched and still serves Status.

The listener needs an exact candidate offer. Its same-user bounded
Pending reads use a memory-only view, with pre-reply and pre-write
checks. Listener abort and drain revoke the view and readiness before
an admitted read task is dropped. The current daemon passes no offer.
These tests qualify the listener, while mounted actor projection is a
separate validation.
