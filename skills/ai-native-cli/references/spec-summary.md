# Agent-Friendly CLI Spec v0.1 -- Full Rule Reference

Use this file when you need to look up a specific rule's details.

## Dimensions Overview

| # | Dimension | Rules | Focus |
|---|-----------|-------|-------|
| 01 | Discoverability | D1-D18 (18) | --help, --brief, --agent/--human, agent/ directory, frontmatter |
| 02 | Output | R1-R3 + O1-O10 (13) | rules[]/skills[]/issue in response, JSON default, --fields, NDJSON |
| 03 | Error | E1-E8 (8) | {error, code, message, suggestion} |
| 04 | Input | I1-I9 (9) | --long-flag, no interactive, type check |
| 05 | Safety | S1-S8 (8) | --yes, --dry-run, input hardening |
| 06 | Exit Code | X1-X9 (9) | 0/1/2/10/11/20/30 semantics |
| 07 | Composability | C1-C7 (7) | stdout=data, stderr=logs, pipe-friendly |
| 08 | Naming | N1-N6 (6) | kebab-case, max 3 levels, reserved flags |
| 09 | Meta | M1-M3 (3) | AGENTS.md, MCP export, changelog |
| 10 | Feedback | F1-F8 (8) | issue subcommand, local storage, data directory, state management |
| 11 | Guardrails | G1-G9 (9) | runtime validation, fail-closed |

## Priority Breakdown

- **P0** (12 rules): O1 O2 O3 E1 E4 E5 E7 E8 X3 X9 C1 C2
- **P1** (34 rules): D1 D3 D4 D7 D9 D11 D12 D13 D15 D16 D17 D18 R1 R2 R3 E6 I1 I2 I4 I5 I8 I9 S1 S4 S8 X1 C6 N4 G1 G2 G3 G6 G8 G9
- **P2** (52 rules): everything else

## Layer Breakdown

- **core** (20 rules): minimum execution contract and safety baseline
- **recommended** (59 rules): machine-friendly ergonomics and richer contracts
- **ecosystem** (19 rules): `agent/`, `skills`, `issue`, and project-level integration

## Certification Levels

- **Agent-Friendly**: All `core` rules pass
- **Agent-Ready**: All `core` + `recommended` rules pass
- **Agent-Native**: All `core` + `recommended` + `ecosystem` rules pass

## New Rules (v0.1.1)

### Response Structure (P1)
- R1: Every command response MUST include `rules[]` (full content from agent/rules/*.md)
- R2: Every command response MUST include `skills[]` (name + description + command)
- R3: Every command response MUST include `issue` (feedback guide)

### Discoverability (P1)
- D15: `--brief` flag outputs agent/brief.md content (one paragraph)
- D16: Default output is JSON (agent mode), `--human` for human-friendly, `--agent` for explicit JSON
- D17: agent/rules/*.md files MUST have YAML frontmatter with `name` and `description`
- D18: agent/skills/*.md files MUST have YAML frontmatter with `name` and `description`

### Feedback (P2)
- F7: Issue data stored in dedicated directory (`~/.{toolname}/issues/`)
- F8: Issues have state management (open/in-progress/resolved/closed)

## P2 Rules Quick Reference

### Output (P2)
- O4: `--fields` for field filtering (saves tokens)
- O5: Empty result returns `[]`, not error
- O6: `--human` flag for human-friendly output
- O7: Multiple results wrapped in JSON array
- O8: Pagination includes total/page/has_more
- O9: NDJSON for streaming large datasets
- O10: Auto-detect TTY, pipe mode = auto JSON

### Input (P2)
- I3: `--json-input` for batch operations via stdin
- I6: Boolean flags explicit: --verbose / --no-verbose
- I7: Array params: --tag a --tag b or --tag a,b

### Safety (P2)
- S2: Destructive ops default to deny without --yes
- S3: `--dry-run` previews impact without executing
- S5: CLI must not auto-update itself
- S6: --describe schema marks destructive commands
- S7: Even with --quiet, destructive ops still require --yes

### Exit Code (P2)
- X2: 1 = general error (fallback)
- X4: 10 = auth failure
- X5: 11 = permission denied
- X6: 20 = resource not found
- X7: 30 = conflict/precondition failed
- X8: Exit code corresponds to JSON error code

### Composability (P2)
- C3: stdout can be piped to next command
- C4: `--quiet` suppresses non-essential stderr
- C5: Upstream --json output feeds downstream --json-input
- C7: Idempotent: same command + same args = same result

### Naming (P2)
- N1: Consistent noun+verb or verb+noun pattern
- N2: Flags use kebab-case (--output-format not --outputFormat)
- N3: Max 3 command levels (mycli resource action)
- N5: `--help` for human-friendly text
- N6: `--version` outputs semver

### Meta (P2)
- M1: AGENTS.md at project root
- M2: Optional MCP tool schema export
- M3: Changelog marks breaking changes

### Feedback (P2)
- F1: `issue` subcommand for feedback
- F2: Structured submission with version/context/exit code
- F3: Categories: bug/requirement/suggestion/bad-output
- F4: Issues stored locally, no external service dependency
- F5: `issue list` / `issue show <id>` queryable
- F6: Issues have open/resolved status

### Guardrails (P2)
- G4: Operation whitelist (read/write/delete per command)
- G5: Output redacts PII (email, phone, ID numbers)
- G7: Rate/volume limits on batch operations
