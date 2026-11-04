# Native command wire boundary follow-up review

Actual Claude Code, `--permission-mode auto`, model `claude-sonnet-5-5`.
Session: `4e0b1c7f-191d-44ec-ad9a-6444edb509f1`.

**Verdict: APPROVE. No blockers.**

Reviewer: Claude Sonnet 5.5 (`claude-sonnet-5-5`), fresh-context read-only session. I ran no Cargo and no tests, and I make no claim that they pass or that the live run succeeds.

**`validate_submit`** (`command_wire.rs:471-487`)
- The change is the one line at 483, from `argv[0].first() != Some(&b'/')` to `argv[0].is_empty()`. The NUL-in-any-arg check, `argv.len() > 128`, empty argv, the `MAX_BYTES` total and the stdin limit are all unchanged.
- `validate_submit` runs at both boundaries. `encode_request` calls it at line 491. `decode_request` reaches it through the Submit body decode, and `submit_digest` also calls it at line 774. So the relaxation is symmetric, and the digest can't be computed over input that encode or decode would refuse.
- Nothing else in `command_*.rs` still assumes a leading `/`. The only hits are the unrelated `NoncanonicalExecutable` detail check and tests.

**Unauthorized-shell and bypass analysis**
- The client never constructs `Shell`. The wire `Submit` carries only argv bytes, and the daemon builds `CommandRequest::Exec { argv }` at `command_engine.rs:603`. A bare name, `./x` or `$(…)` is a literal argv0 or argument. It is never put through a shell. Resolution in `resolve.rs` is `Path::join` plus `canonicalize`, with no expansion.
- The server order is `resolve_exec` and then `check_execute_request`. After resolution the request is `ResolvedExec` with an absolute canonical `executable`. `check_execute_request` then calls `validate_request(…, true)`, `check_command` (deny/allow patterns, matched on the canonical path and also on the invoked path as an alias) and `require_canonical_exec`. `submit` re-runs `check_execute_request`, so the check is repeated before admission.
- If a raw relative `Exec` ever reached `check_execute_request` without resolving, it would still fail. `executable_names` returns `InvalidRequest` unless argv0 starts with `/`, and `inspect_canonical_exec` returns `Unresolvable` for the same reason. So dropping the wire-level absolute requirement doesn't open a path around server policy. The server is still the only authority.
- PATH comes from the daemon's own environment (`command_engine.rs:354`), not from the client. Bare-name and relative lookup is anchored on `account.cwd`, so a client can't pick the search path or working directory.

**New tests**
- The round-trip test covers bare, `./`, `subdir/`, `../`, non-UTF-8 and metacharacter argv0, plus an empty tail argument and non-UTF-8 and quote/glob/`$()` arguments. It asserts exact argv equality, request equality and re-encode byte equality. That checks canonical encoding, not only decodability.
- The negative test covers empty argv0 and NUL in argv0 or a later argument, on both encode and a mutated decode.
- Removing `b"relative"` from the invalid list in `combined_envelope_and_decoded_bounds` is correct. The list still has empty argv, `vec![vec![]]`, a NUL path and 129 args.

**Non-blocking notes**
1. The empty-argv0 case is rejected at three layers (wire, `validate_request`, `inspect_canonical_exec`). That's fine as defense in depth.
2. The wire test accepts argv0 up to `MAX_BYTES`. The engine caps it at 4096 and `resolve` at 4095, which fails closed with `InvalidRequest`. A server-side test that an overlong bare name maps cleanly to a refusal would be nice, but it isn't required.
3. The diff adds a stray blank line after `validate_error`. It's cosmetic, and `rustfmt` may reflow it.
4. `validate_error` accepts a detail only for `NoncanonicalExecutable` or a closed reason string. The reason is checked per code by `diagnostic_reason`, and a pairing test covers every code and reason combination. That matches the approved design and I saw no problem.

**Root should run**
- The affected native suite: `command_wire`, `command_engine`, `command_cli`.
- The repeat live bare-command run.
- Clippy with `-D warnings`, per project policy.

Disposition: approved within the existing bare-name design. Root runs affected native tests and repeats live relay validation before publication. The original live trial failed locally at command_argv and is not presented as a transport or execution pass.
