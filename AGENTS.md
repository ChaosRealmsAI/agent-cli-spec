# agent-cli-lint

Agent-Friendly CLI Spec v0.1 compliance checker.

## What it does

Checks any CLI tool against 98 rules across 11 dimensions of the Agent-Friendly CLI Spec.
Outputs structured JSON reports with pass/fail/skip status, layer coverage, and certification level.

## Quick start

```bash
agent-cli-lint check <cli> --json          # Full check
agent-cli-lint check <cli> --layer core    # Core contract only
agent-cli-lint check <cli> --priority p0   # P0 rules only
agent-cli-lint check <cli> --dimension 03  # Single dimension
agent-cli-lint check <cli> --rule O1       # Single rule
```

## Commands

| Command | Description |
|---------|-------------|
| `check` | Run compliance checks |
| `snapshot` | Save schema snapshot for stability checks |
| `diff` | Compare current vs saved snapshot |
| `ai-prompts` | Generate AI review prompts for non-automatable rules |
| `issue` | Feedback system (create/list/show) |
| `skills` | View available skills |

## Certification levels

- **Agent-Friendly**: All `core` rules pass
- **Agent-Ready**: All `core` + `recommended` rules pass
- **Agent-Native**: All `core` + `recommended` + `ecosystem` rules pass

## Dependencies

bash + jq
