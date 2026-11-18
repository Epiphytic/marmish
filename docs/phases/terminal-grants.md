# Explicit local grants for interactive shells

This increment follows the [interactive roadmap](interactive-peers-roadmap.md).
Treat a terminal grant as permission to run a shell with the effective
nonroot OS account's permissions. Executable-path restrictions still apply
to ordinary command admission. They do not restrict commands typed into an
interactive shell. Keep every existing profile default off for Terminal.
No PTY, daemon reload, relay or session is activated by this increment.

## Config and local inventory

Extend existing strict `command_config::RawConfig` with the optional
defaulted object `"terminal": {"clients": []}`. Deny unknown and duplicate
fields in the object and reject null. An explicit object requires `clients`.
Use the existing canonical account parser, set representation and normalized
`execution_clients`.
Allow at most 256 terminal keys. Reject invalid keys, duplicate normalized
keys, a nonempty list on a client-only profile, and any key outside the
same snapshot's execution clients. Validate the raw list length before
walking it. Explicit empty terminal configuration is valid in all roles.
Do not consult glob patterns or inventory history. No new identity or
field is published.

Expose one read-only `CommandConfig::terminal_allowed(&Account)` query.
It returns true only in the Server or Both role with current execution and
terminal grants for the same canonical key. The local inventory uses it. The eventual supervisor will
consult it on startup and from validated grant snapshots at each privileged
operation. That phase reviews safe reload, revocation, process ownership and
restart behavior against actual daemon entry points. Until then reload has
no effect and the startup setting grants no reachable remote service.
Unknown configuration continues to fail closed. Older binaries lack the
field, so remove it before rolling back.

Add `terminal_shell_granted: bool` to the local server's client inventory
entry for both list and show. The field describes configured access and
does not claim the runtime is ready. Inventory looks up only the existing
current grant set. Keep policy pages, cursors, counts, history, command
admission and journal schema unchanged. Allow the new binary to decode a
missing field from an old local daemon as false. Keep the inventory version. The old strict client rejects the extra field
from a new daemon with `inventory_response`. Test both cross-version decode
paths. The serialized `npub` must represent the exact key used for the grant
lookup. The existing policy field `interactive_shell_enabled` stays false
until the runtime can enable that capability. Document manual config and
these JSON fields in the README.

A later setup CLI may add a typed grant editor. It needs a separate
review for operator confirmation, atomic updates and no accidental grants
during profile reuse. Existing setup changes preserve the raw JSON object through the existing
`Value` rewrite. In `native_setup::proposed`, remove terminal keys whose
normalized accounts were removed by execution revocation. A later execution
regrant never restores a removed terminal grant. Client role conversion
with remaining terminal clients fails validation before writing. Exercise
`--no-grants` with existing setup semantics and do not broaden the terminal
set. Omit the terminal section from the new-profile template.

## Checks

Test old config and an explicitly empty section, allowed and disallowed
canonical keys, equivalent encodings, duplicate normalized keys, a key
outside the current execution set, each role, unknown and duplicate fields,
null and the maximum-size boundary. Prove an unrestricted Command policy
does not grant Terminal. Verify default false and configured true in list
and show after serializing and decoding actual local replies. Exercise each
existing setup rewrite that touches the config. Run the ordinary command
config, policy, profile onboarding and inventory tests. Add one fast
selector to the existing trusted-branch core workflow only if the
existing inventory/config selections do not already cover it. No separate
workflow or new network fixture is needed for this local change.
