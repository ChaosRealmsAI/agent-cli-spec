---
name: getting-started
description: Quick start guide for new users
---
# Getting Started

## Quick Start

```bash
# Create a note
note add --title "Meeting notes" --body "Discussed Q3 roadmap" --tags work,meeting

# List all notes (default JSON)
note list

# Search by keyword
note search --query "roadmap"

# Show a specific note
note show <id>

# Delete a note (requires --yes)
note delete <id> --yes

# Preview deletion without executing
note delete <id> --dry-run
```

## Pipe Examples

```bash
# Extract all titles
note list | jq -r '.result[].title'

# Count notes by tag
note list | jq '[.result[].tags[]] | group_by(.) | map({tag: .[0], count: length})'
```
