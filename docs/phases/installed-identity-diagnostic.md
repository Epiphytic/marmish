# Installed identity failure diagnostics

This bounded diagnostic increment supports the installed nine-pair validation
phase. Native ARM run `41395347740` passed installation and verification, then
failed server identity initialization after approximately 254 ms. Its existing
reports did not retain the underlying CLI failure category. That evidence does
not establish a confinement or application root cause.

The implementation is restricted to `IdentityError`, identity failure creation
in `initialize_identity`, closed extraction in `failure_diagnostic`, and focused
tests in `tools/test_marmish_installed_state.py`. Process ownership, confinement,
environment construction, cleanup, identity storage and public installation
artifact schemas remain governed by their existing contracts.

## Diagnostic contract

`str(IdentityError)` remains `identity_init_failed`. Only an exact `IdentityError`
instance adds a nested `identity` object to the existing fixed JSON diagnostic
written to stderr. Other exception types cannot opt in through similar
attributes. The existing `error_type` vocabulary now includes `IdentityError`.

The nested object contains only these selected facts:

| Field | Allowed values |
| --- | --- |
| `reason` | `timed_out`, `output_limited`, `cleanup_incomplete`, `nonzero_exit`, `invalid_identity_output`, or `other` |
| `returncode` | Exact integer in `[-255, 255]`, excluding booleans, or null |
| `signal` | Negated validated return code only when it is in `[-64, -1]`; otherwise null |
| `timed_out`, `output_limited` | Exact boolean or null |
| `cleanup` | `complete`, `incomplete`, or `other` |
| `frontend_category` | A member of the frozen `marmish_release_acceptance.DIAGNOSTIC_ERRORS` set, `none`, or `other` |

Failure precedence is timeout, output limit, incomplete cleanup, then nonzero
exit. JSON parsing or account-schema failures use `invalid_identity_output`.
The incomplete-cleanup returned-result case is defensive: the current
`run_bounded` raises on uncertain cleanup instead of returning that result.
A timeout's signal can describe the harness's own termination and does not
identify the original cause.

Frontend categories match only exact byte equality with one complete line of
the form `marmish: CATEGORY` followed by a newline. Empty bytes produce `none`;
absent or malformed stderr, unknown categories, extra lines and debug output
produce `other`. The frozen vocabulary already covers the pinned frontend's
transport, RPC, key-storage and fallback categories. It is reused without
runtime source scanning or expansion from child output.

The exception stores seven selected attributes and never stores the full
process result. Extraction validates types, ranges and membership again, and
derives the signal again rather than trusting an attribute. No raw stderr,
stdout, exception text, environment, argument vector, paths, process IDs, keys
or credentials are exported. This diagnostic does not infer which startup
operation failed or relax any confinement requirement.

## Review and validation

The actual independent Claude Code reviews used model `claude-opus-5-5` with
`--permission-mode auto`, limited to source inspection and offline mocked tests.

- [Initial design review](../reviews/installed-identity-diagnostic-design-initial.json)
  requested changes because a transport-only category list would hide other
  frontend failures. That verdict is preserved.
- [Approved design review](../reviews/installed-identity-diagnostic-design-final.json)
  accepts reuse of the existing frozen vocabulary, the empty-stderr sentinel
  and exact-class extraction.
- [Approved code review](../reviews/installed-identity-diagnostic-code.json)
  found no blocking issues. Useful follow-up tests pin the selected attribute
  set and absent/non-dictionary result behavior; production source did not
  change after approval.

Final selected offline validation passed **9 tests and 8 subtests**. It covers
every frozen category, unknown and extra-line rejection, reason precedence,
numeric and boolean validation, mutated-attribute revalidation, absent results,
raw-result exclusion and existing identity initialization behavior. Python
compilation also passed. No new native run or successful ARM identity
initialization is claimed by this increment; the next native run must provide
that evidence.
