# Identity helper fixture review

Linux ARM64 candidate run `45694419397`, job `139488227966`, failed the
identity helper size test with `BrokenPipe`. The shell fixture could exit
before the test parent wrote its request because it never read stdin.
Waiting for the old helper to exit before writing reproduced the error.

Claude Sonnet approved the test-only design in auto mode in session
`32447a40-d681-4d07-be48-13ac7b184e48`. Both fixtures now drain stdin
before emitting their response. The helper implementation, deadlines and size
assertions are unchanged.

Claude Sonnet approved the staged code and ontology in auto mode in session
`0539bace-08dc-4ee7-8629-efc8856b795c` against tree
`615c417d5a9b04b34eb5fa9b05414c49dc4abb59`. Both shell fixtures completed
200 combined exchanges. The exact Rust test passed 150 consecutive local
invocations. Rust formatting, the staged diff and ontology checks passed.
The release workflow still requires a fresh successful candidate on all
platforms and the complete installed qualification before publication.
