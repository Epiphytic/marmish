# Terminal enrollment cleanup diagnostics

Keep the original enrollment error when closing an already failed flight. Log
failed cleanup with the fixed `cleanup_failed` category and a static stage
that identifies the cleanup site. Always perform the existing close exactly
once. Return capacity only after successful owned cleanup. Preserve existing
reaper failure reporting and cancellation behavior.

Claude Code Sonnet approved this amendment in Auto review
`501ef4db-7462-4c00-9991-9da9c60ec38a`. The review identified the previously
silent inbox cleanup and requested tests for primary error preservation, room
permit custody, and the resulting client-visible refusal category.
