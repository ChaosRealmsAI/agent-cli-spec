# agent-cli-lint

Rust implementation of the strict Agent-Friendly CLI development gate.

## Run

```bash
cargo run -- check <cli> --json
```

## Behavior

- strict gate: every selected rule must be `pass`
- any `fail`, `skip`, `ai_review`, or `warn` returns exit code `1`
- project metadata and rule registry live under:
  - `agent/`
  - `data/`
