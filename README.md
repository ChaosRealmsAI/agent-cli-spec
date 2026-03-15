# ai-native-cli

A design spec for building CLI tools that AI agents can safely and reliably use.

Covers structured JSON output, error handling, input contracts, safety guardrails, exit codes, and agent self-description across 98 rules in 11 dimensions.

## Why

Most CLIs were designed for humans — colorized output, interactive prompts, ambiguous exit codes. When an AI agent calls these tools, it can't parse the output, gets stuck on prompts, or silently misinterprets errors. This spec defines what a CLI needs to be a reliable building block in agentic workflows.

## Install

```bash
# Claude Code
npx skills add zhuanz/agent-cli-spec

# Or manually
cp -r ai-native-cli/ ~/.claude/skills/ai-native-cli/

# OpenAI Codex
cp -r ai-native-cli/ .agents/skills/ai-native-cli/
```

## What's inside

| File | Purpose |
|------|---------|
| `ai-native-cli/SKILL.md` | The spec — 98 rules across 11 dimensions |
| `ai-native-cli/references/audit.md` | Audit protocol — Agent-driven compliance verification |

## Certification levels

| Level | Layer | Rules |
|-------|-------|-------|
| Agent-Friendly | core | 20 rules — stable execution contract |
| Agent-Ready | + recommended | 59 rules — self-describing, pipe-friendly |
| Agent-Native | + ecosystem | 19 rules — identity, skills, feedback loop |

## Compatibility

Built on the [Agent Skills](https://agentskills.io) open standard. Works with:

- Claude Code
- OpenAI Codex
- Cursor
- GitHub Copilot
- Gemini CLI
- and 30+ other agents

## License

MIT
