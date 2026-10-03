# ACP schema fixtures

- `schema-v1.23.0.json` and `meta-v1.23.0.json` are verbatim copies of
  `schema/v1/schema.json` and `schema/v1/meta.json` from
  https://github.com/agentclientprotocol/agent-client-protocol at commit
  `f05af18d9708f31c85fa62e172ac0968df042cf0` (schema-v1.23.0).
  - schema sha256 `3c17bd6385d90cf672d8a661fddc359d73422cf8b8ce6865213d25cfd4c0eca7`
  - meta sha256 `061edb6efa8fb2aa2792459a86ec7268de5fe665bba48b2ffe7939df01481f88`
- `messages/*.json` are hand-written samples, one per request, response and
  notification Pocket-Codex models: `{"def": "<$defs name>", "frame": {...}}`.
- `opencode-2.0.18-initialize.json` is the `initialize` result measured
  against OpenCode 2.0.18 (docs/acp-integration/research.md §3.5).
- `replays/*.jsonl` are hand-written `session/update` params in the shapes of
  Claude, Codex and OpenCode replays; they are replaced by recorded,
  redacted sequences in M10.

To upgrade the schema, replace both schema files in a separate change and
make `tests/acp_schema.rs` pass.
