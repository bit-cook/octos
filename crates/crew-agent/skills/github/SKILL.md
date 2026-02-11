---
name: github
description: Interact with GitHub using the gh CLI for issues, PRs, CI runs, and API queries.
requires_bins: gh
---

# GitHub CLI (gh)

Use the `gh` command-line tool for GitHub operations.

## Pull Requests

```bash
# List open PRs
gh pr list

# View PR details
gh pr view 123

# Check PR status (CI, reviews)
gh pr checks 123

# Create a PR
gh pr create --title "feat: add feature" --body "Description here"

# Merge a PR
gh pr merge 123 --squash
```

## Issues

```bash
# List issues
gh issue list

# View issue
gh issue view 42

# Create issue
gh issue create --title "Bug: crash on startup" --body "Steps to reproduce..."

# Close issue
gh issue close 42
```

## CI/CD Runs

```bash
# List recent workflow runs
gh run list

# View run details
gh run view 12345

# Watch a running workflow
gh run watch 12345

# Re-run failed jobs
gh run rerun 12345 --failed
```

## API Queries

```bash
# Get repo info
gh api repos/{owner}/{repo}

# List releases
gh api repos/{owner}/{repo}/releases --jq '.[0].tag_name'

# Search code
gh api search/code -f q='filename:Cargo.toml org:myorg' --jq '.items[].repository.full_name'

# GraphQL query
gh api graphql -f query='{ viewer { login } }'
```

## Tips

- Use `--json` flag for machine-readable output
- Pipe to `jq` for filtering: `gh pr list --json number,title | jq '.[].title'`
- Set `GH_TOKEN` env var for authentication in CI
