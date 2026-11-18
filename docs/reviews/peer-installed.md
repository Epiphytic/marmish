# Installed Peer umbrella design review

Claude Code Sonnet 5.5 in Auto mode approved the umbrella design in session
`a0c57eca-bcdc-49ba-999a-724d4dcb5939`. Session
`0a1dc183-ca78-4c34-ba63-ce97a718f455` approved the final design with SHA-256
`d7181a0adfb8293fea3a6c8036a3fe654766bb75062adf725eeb9f963d68aff3`,
the agent specification update and the two supersession notes.

The final review verified the join-before-decision behavior, Manual Pending
for an unlisted founder even when an allow-list has enabled automatic
acceptance for others, and separate revocation and membership departure.
It also checked the locked MDK source. The public account opener uses the
default SQLite VFS. Applying cipher hardening to an already open connection
does not create an MDK account. A default-VFS dispatcher would need its
own reviewed startup ordering and platform proof.

The approved design permits separate reviewed work on disabled config,
foreground identifier confirmation and local inventory. Installed owners,
quota mounting, relay egress and support claims each require their relevant
gates. Review was read-only with no live tests. The phase remains an
umbrella design for later increments.
