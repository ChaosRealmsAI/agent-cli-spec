---
name: ai-native-cli
description: >
  AI-Native CLI design spec. Use when building CLI tools, designing command-line
  interfaces, or scaffolding new CLI projects. Covers structured output, error
  handling, input contracts, safety guardrails, and exit codes.
---

# Agent-Friendly CLI Spec v0.1

When building or modifying CLI tools, follow these rules to make them safe and
reliable for AI agents to use.

## Core Philosophy

1. **Agent-first** -- default output is JSON; human-friendly is opt-in via `--human`
2. **Agent is untrusted** -- validate all input at the same level as a public API
3. **Fail-Closed** -- when validation logic itself errors, deny by default
4. **Verifiable** -- every rule is written so it can be automatically checked

## Layer Model

This spec now uses two orthogonal axes:

- **Layer** answers rollout scope: `core`, `recommended`, `ecosystem`
- **Priority** answers severity: `P0`, `P1`, `P2`

Use layers for migration and certification:

- **core** -- execution contract: JSON, errors, exit codes, stdout/stderr, safety
- **recommended** -- better machine UX: self-description, explicit modes, richer schemas
- **ecosystem** -- agent-native integration: `agent/`, `skills`, `issue`, inline context

Certification maps to layers:

- **Agent-Friendly** -- all `core` rules pass
- **Agent-Ready** -- all `core` + `recommended` rules pass
- **Agent-Native** -- all layers pass

## Output Mode

Default is agent mode (JSON). Explicit flags to switch:

```bash
$ mycli list              # default = JSON output (agent mode)
$ mycli list --human      # human-friendly: colored, tables, formatted
$ mycli list --agent      # explicit agent mode (override config if needed)
```

- **Default (no flag)** — JSON to stdout. Agent never needs to add a flag.
- **--human** — human-friendly format (colors, tables, progress bars)
- **--agent** — explicit JSON mode (useful when env/config overrides default)

## agent/ Directory Convention

Every CLI tool MUST have an `agent/` directory at its project root. This is the
tool's identity and behavior contract for AI agents.

```
agent/
├── brief.md          # One paragraph: who am I, what can I do
├── rules/            # Behavior constraints (auto-registered)
│   ├── trigger.md    # When should an agent use this tool
│   ├── workflow.md   # Step-by-step usage flow
│   └── writeback.md  # How to write feedback back
└── skills/           # Extended capabilities (auto-registered)
    └── getting-started.md
```

### File Format

**agent/brief.md** — plain text, one paragraph. No frontmatter needed.

**agent/rules/*.md** — each file MUST have YAML frontmatter:

```yaml
---
name: trigger
description: When should an agent use this tool
---
(content here)
```

**agent/skills/*.md** — each file MUST have YAML frontmatter:

```yaml
---
name: getting-started
description: Quick start guide for new users
---
(content here)
```

The `name` field is the canonical identifier. The `description` field tells agents
when/why to read this rule or skill.

### Auto-Registration

Drop a `.md` file into `agent/rules/` or `agent/skills/` and it is automatically
registered. The CLI reads these directories at runtime. No code changes needed.

## Four Levels of Self-Description

### Level 1: --brief (business card, injected into agent config)

The smallest possible context. Output of `--brief` gets synced into CLAUDE.md /
AGENTS.md by `cli-toolkit sync-agent`, so agents always know this tool exists.

```bash
$ mycli --brief
mycli — task manager, add/list/show/done for local tasks
```

Source: `agent/brief.md`. Just one paragraph. Always in agent's system prompt.

### Level 2: Every Command Response (always-on context)

EVERY command's JSON output MUST include three fixed fields:

```json
{
  "result": { "id": 1, "title": "Buy milk", "status": "todo" },

  "rules": [
    {"name": "trigger",   "content": "full content of trigger.md"},
    {"name": "workflow",  "content": "full content of workflow.md"},
    {"name": "writeback", "content": "full content of writeback.md"}
  ],
  "skills": [
    {"name": "getting-started", "description": "Quick start guide", "command": "mycli skills getting-started"},
    {"name": "batch-import",    "description": "Import from CSV",   "command": "mycli skills batch-import"}
  ],
  "issue": "Any problem, bad output, or confusion — run: mycli issue create --type <bug|requirement|suggestion|bad-output> --message '...'"
}
```

- **rules** — full `.md` content inline (push: agent must know)
- **skills** — name + description + command (pull: agent reads on demand)
- **issue** — one-line guide, always present (fallback: can't figure it out? report it)

These three fields form a closed loop:
  rules tell you how -> skills teach you more -> issue catches what you can't handle

### Level 3: --help (full self-description)

Complete identity + capabilities. Only called for deep exploration.

```json
{
  "help": "mycli — full description from agent/brief.md",
  "commands": [
    {"name": "add",  "description": "Create a new task"},
    {"name": "list", "description": "List all tasks"},
    {"name": "done", "description": "Mark task as done (destructive)"}
  ],
  "rules": [
    {"name": "trigger",   "content": "..."},
    {"name": "workflow",  "content": "..."},
    {"name": "writeback", "content": "..."}
  ],
  "skills": [
    {"name": "getting-started", "description": "Quick start guide", "command": "mycli skills getting-started"}
  ],
  "issue": "Any problem — run: mycli issue create ..."
}
```

### Level 4: skills <name> (on-demand deep dive)

```bash
$ mycli skills getting-started
{
  "name": "getting-started",
  "content": "full content of getting-started.md",
  "rules": [ ... same rules for context ... ]
}
```

### Summary: Information Architecture

```
--brief              → always in agent's prompt (via sync-agent)
every command        → data + rules + skills list + issue (always attached)
--help               → brief + commands + rules + skills + issue (first contact)
skills <name>        → full skill content + rules (on demand)
```

## P0 Rules (mandatory, must all pass)

### Output
- **O1** Default output is JSON (agent mode). No flag needed.
- **O2** JSON output MUST pass `jq .` validation
- **O3** JSON schema MUST NOT change within the same version

### Response Structure
- **R1** Every command response MUST include `rules[]` (full content from agent/rules/*.md)
- **R2** Every command response MUST include `skills[]` (name + description + command)
- **R3** Every command response MUST include `issue` (feedback guide)

### Error
- **E1** All errors MUST output `{ "error": true, "code": "...", "message": "...", "suggestion": "..." }` to stderr
- **E4** Error MUST have a machine-readable `code` (e.g. `MISSING_REQUIRED`, `AUTH_EXPIRED`)
- **E5** Error MUST have a human-readable `message`
- **E7** On error, NEVER enter interactive mode -- print error and exit immediately
- **E8** Error codes are API contracts -- MUST NOT rename across versions

### Exit Code
- **X3** Parameter/usage errors MUST exit 2 (not 1)
- **X9** Failures MUST exit non-zero -- never exit 0 then report error in stdout

### Composability
- **C1** stdout is for data ONLY
- **C2** logs, progress, warnings go to stderr ONLY

## P1 Rules (important)

### Self-Description
- `--brief` outputs `agent/brief.md` content (one paragraph)
- `--help` outputs structured JSON with brief, commands, rules, skills, issue
- `agent/rules/` with trigger.md + workflow.md + writeback.md
- `skills` subcommand: list all / show one with full content + rules context

### Input
- All params MUST have `--long-flag` names
- Missing required param returns structured error, never enters interactive
- Type mismatch returns exit 2 + structured error

### Safety
- Destructive ops require `--yes` confirmation
- Reject `../../` path traversal, control chars, encoded strings

### Composability
- In pipe mode, never output prompts, confirmations, or spinners

### Naming — Reserved Flags

| Flag | Semantics | Notes |
|------|-----------|-------|
| `--agent` | JSON output (default) | Explicit override |
| `--human` | Human-friendly output | Colors, tables, formatted |
| `--brief` | One-paragraph identity | For sync into agent config |
| `--help` | Full self-description JSON | Brief + commands + rules + skills + issue |
| `--version` | Semver version string | |
| `--yes` | Confirm destructive ops | Required for delete/destroy |
| `--dry-run` | Preview without executing | |
| `--quiet` | Suppress stderr output | |
| `--fields` | Filter output fields | Save tokens |

### Error
- Error includes `suggestion` field telling agent what to do next

### Guardrails
- Validate params against schema at runtime (type/range/enum)
- Detect API key / password / token patterns in args, reject execution
- Reject sensitive file paths (*.env, *.key, *.pem, *secret*)
- Reject shell metacharacters in arguments (; | && $() backticks)
- When validation logic itself fails, default to deny

## Exit Code Table

```
0   success         10  auth failed       20  resource not found
1   general error   11  permission denied 30  conflict/precondition
2   param/usage error
```

## Error Format

```json
{
  "error": true,
  "code": "AUTH_EXPIRED",
  "message": "Access token expired 2 hours ago",
  "suggestion": "Run 'mycli auth refresh' to get a new token"
}
```

## Quick Implementation Checklist

When writing a CLI tool from scratch, implement in this order:

1. Create `agent/` directory: `brief.md`, `rules/trigger.md`, `rules/workflow.md`, `rules/writeback.md`
2. Default output is JSON — no `--json` flag needed
3. `--human` flag switches to human-friendly format
4. Every command response appends: rules[] + skills[] + issue
5. `--brief` reads and outputs `agent/brief.md` content
6. `--help` returns JSON: help + commands[] + rules[] + skills[] + issue
7. `skills` subcommand: list all / show one with full content
8. Error handler: `{ error, code, message, suggestion }` to stderr
9. Exit codes: 0 success, 2 param error, 1 general, 20 not found, 30 conflict
10. Guardrails: reject secrets, path traversal, shell metacharacters
11. `--yes` guard on destructive operations
12. `issue` subcommand for feedback (create/list/show/close/transition)

## Issue System Specification

Every CLI tool MUST have a built-in issue system for agents to report problems,
request features, and track feedback.

### Storage

Issues are stored in a dedicated directory:

```
~/.{toolname}/issues/       # or {TOOL_DIR}/issues/
├── 001.json
├── 002.json
└── 003.json
```

Each issue is a single JSON file named `{id}.json`.

### Issue Fields

```json
{
  "id": "001",
  "type": "bug",
  "status": "open",
  "message": "list command returns empty when tasks exist",
  "context": {
    "version": "0.1.0",
    "command": "mycli list --json",
    "exit_code": 0
  },
  "created_at": "2026-03-14T00:00:00Z",
  "updated_at": "2026-03-14T00:00:00Z"
}
```

Required fields:
- **id** — unique identifier
- **type** — one of: `bug`, `requirement`, `suggestion`, `bad-output`
- **status** — one of: `open`, `in-progress`, `resolved`, `closed`
- **message** — description of the issue
- **context** — object with `version`, `command`, `exit_code`
- **created_at** — ISO 8601 timestamp
- **updated_at** — ISO 8601 timestamp

### State Management

| Command | Description |
|---------|-------------|
| `issue create --type <type> --message <msg>` | Create a new issue |
| `issue list [--type <type>] [--status <status>]` | List issues, filterable |
| `issue show <id>` | Show single issue detail |
| `issue close <id>` | Close an issue |
| `issue transition <id> --status <status>` | Change issue status |

### Queryable

Issues MUST be filterable by `--type` and `--status`:

```bash
$ mycli issue list --type bug --status open
$ mycli issue list --status resolved
