---
name: skill-store
description: Browse and install community skills from the skill registry.
always: true
---

# Skill Store

IMPORTANT: You MUST use the shell tool to run `crew skills search` commands. Do NOT use web search or try to look up skills online. The registry is accessed via the CLI command only.

When the user asks to browse skills, install skills, find available skills, show skill store, or similar (including Chinese: 技能商店, 安装技能, 查看技能, 浏览技能, 搜索技能):

## Step 1: Search the Registry

ALWAYS run this shell command first — do NOT delegate to a subagent:

```bash
crew skills search --cwd {{CWD}}
```

To filter by keyword (e.g. mofa, slides, video):

```bash
crew skills search mofa --cwd {{CWD}}
```

Show the command output directly to the user.

## Step 2: Install

When the user picks a package, run:

```bash
crew skills install <user/repo> --cwd {{CWD}}
```

Example:
```bash
crew skills install mofa-org/mofa-skills --cwd {{CWD}}
```

Options:
- `--force` to overwrite existing skills
- `--branch <tag>` for a specific version

## Step 3: Verify

```bash
crew skills list --cwd {{CWD}}
```

Tell the user what was installed and any requirements (e.g. API keys).
