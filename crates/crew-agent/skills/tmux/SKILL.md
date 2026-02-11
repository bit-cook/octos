---
name: tmux
description: Remote-control tmux sessions for interactive CLIs by sending keystrokes and scraping output.
requires_bins: tmux
---

# tmux Automation

Control tmux sessions to run interactive CLI tools, monitor output, and orchestrate parallel tasks.

## Creating Sessions

```bash
# Create a named session
tmux new-session -d -s my-session

# Create with a specific command
tmux new-session -d -s build "cargo build --workspace"
```

## Sending Input

```bash
# Send a command (type + Enter)
tmux send-keys -t my-session "cargo test" Enter

# Send Ctrl+C to interrupt
tmux send-keys -t my-session C-c

# Send text without Enter
tmux send-keys -t my-session "partial input"
```

## Reading Output

```bash
# Capture current pane content
tmux capture-pane -t my-session -p

# Capture with history (last 1000 lines)
tmux capture-pane -t my-session -p -S -1000
```

## Session Management

```bash
# List sessions
tmux list-sessions

# Kill a session
tmux kill-session -t my-session

# Check if session exists
tmux has-session -t my-session 2>/dev/null && echo "exists"
```

## Orchestration Pattern

Use tmux to run parallel tasks:

```bash
# Start multiple build agents
tmux new-session -d -s agent-1 "crew run 'Fix auth bug'"
tmux new-session -d -s agent-2 "crew run 'Add tests for API'"

# Monitor progress
tmux capture-pane -t agent-1 -p | tail -5
tmux capture-pane -t agent-2 -p | tail -5
```

## Tips

- Always use `-d` (detached) when creating sessions from scripts
- Use `capture-pane` to check command output before sending next input
- Wait for output to settle before scraping (use `sleep` if needed)
