---
name: skill-creator
description: Create or update custom skills with scripts, references, and assets.
---

# Skill Creator

Create custom skills in `.crew/skills/{name}/SKILL.md`.

## Skill Structure

```
.crew/skills/
  my-skill/
    SKILL.md          # Required: instructions + frontmatter
    scripts/           # Optional: helper scripts
    references/        # Optional: reference docs
```

## SKILL.md Format

```markdown
---
name: my-skill
description: Brief description of what this skill does
always: false
requires_bins: docker,kubectl
requires_env: GITHUB_TOKEN
---

# Skill Title

Instructions for the agent on how to use this skill.
Include examples, commands, and best practices.
```

## Frontmatter Fields

| Field | Required | Description |
|---|---|---|
| `name` | Yes | Skill identifier |
| `description` | Yes | One-line description (shown in skill index) |
| `always` | No | `true` to auto-load in every prompt (default: false) |
| `requires_bins` | No | Comma-separated binaries that must be on PATH |
| `requires_env` | No | Comma-separated env vars that must be set |

## Loading Behavior

- Skills with `always: true` are included in every system prompt
- Other skills appear in the skill index (XML summary)
- The agent can read any skill on demand via `read_file`
- Workspace skills (`.crew/skills/`) override built-in skills with the same name

## Best Practices

1. Keep skills concise (under 200 lines)
2. Include concrete examples, not abstract theory
3. Use `requires_bins` to gate skills needing external tools
4. Set `always: true` sparingly (adds to every prompt)
