---
name: cron
description: Schedule reminders and recurring tasks using the cron tool.
always: true
---

# Cron Scheduling

Use the `cron` tool to schedule reminders and recurring tasks.

## Actions

### Add a recurring job
```json
{"action": "add", "name": "standup", "message": "Time for daily standup!", "every_seconds": 86400}
```

### Add a cron-expression job
```json
{"action": "add", "name": "morning", "message": "Good morning check-in", "cron_expr": "0 0 9 * * * *"}
```

### Add a one-time job
```json
{"action": "add", "name": "reminder", "message": "Meeting in 5 minutes", "at_ms": 1707552000000}
```

### List jobs
```json
{"action": "list"}
```

### Remove a job
```json
{"action": "remove", "job_id": "abc12345"}
```

### Enable/disable a job
```json
{"action": "enable", "job_id": "abc12345"}
{"action": "disable", "job_id": "abc12345"}
```

## Delivery

To deliver responses to a specific channel:
```json
{"action": "add", "name": "alert", "message": "Check metrics", "every_seconds": 3600, "channel": "telegram", "chat_id": "123456"}
```

## Cron Expression Format

Standard 7-field cron: `sec min hour day-of-month month day-of-week year`

| Expression | Meaning |
|---|---|
| `0 0 9 * * * *` | Every day at 9:00 AM |
| `0 0 */2 * * * *` | Every 2 hours |
| `0 30 9 * * 1-5 *` | Weekdays at 9:30 AM |
| `0 0 0 1 * * *` | First of every month |
