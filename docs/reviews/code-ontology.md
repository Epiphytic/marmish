# Code ontology review

Claude Sonnet 5.5 reviewed the phase design and the integrated implementation
in Claude Code auto mode. Scope included the extraction and model, the public
CI check, the exact-ref release gates, and the installer cutover.

- **Design approved.** The independent review required exact grammar pins,
  closed input classification, Git object reads, deterministic symbol keys,
  and a trusted checker for candidate objects. A follow-up design review
  approved checking the rendered installer against its tagged source
  template while indexing the native template in full.
- **Initial code review blocked.** The revised implementation excludes
  runtime literals from call targets, reports omitted declarations, tests
  model validation with NVIDIA's pinned schema, anchors Git operations at the
  repository root, and tests the real SQLite export command.
- **Integrated code approved.** Claude inspected both release gates, installer
  rendering, the staged graph, and the workflow's permissions. It ran 11
  ontology tests, 19 release policy tests, the real SQLite export, and three
  deterministic freshness checks.
- **Final follow-up approved.** Claude verified review tree
  `8f7ad2e56f16e9ba7cd1fe10bdc759ba65c5ccb6`, ran 12 ontology tests, and
  confirmed the final graph passed. This revision adds coverage for parser
  recovery, omitted declarations, and a tagged `-rc.N` installer. It
  also excludes numeric literal receivers and dynamic subscripts from
  call targets. This review record was added afterward and is outside the
  generator's code inputs.

See [validation results](../validation/code-ontology.md) for local evidence.
