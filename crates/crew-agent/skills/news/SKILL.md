---
name: news
description: Fetch categorized news digests and subscribe to daily delivery.
always: true
---

# News Digest

Use the `news_digest` tool to fetch and synthesize categorized news into a Chinese digest. Sources include Google News RSS, Hacker News API, Yahoo News, Substack, and Medium.

## On-Demand Usage

When the user asks for news (e.g. "/news", "今日新闻", "give me news"):

### All categories
```json
{"categories": []}
```

### Specific categories
```json
{"categories": ["politics", "tech", "international"]}
```

## Available Categories

| Alias | Category | 中文 |
|---|---|---|
| `politics` | US Politics | 美国政治 |
| `international` / `world` | World | 国际新闻 |
| `tech` / `technology` | Technology | 科技 |
| `commerce` / `business` | Business | 商业财经 |
| `social` / `entertainment` | Entertainment | 社会娱乐 |
| `science` | Science | 科学 |
| `health` | Health | 健康 |
| `sports` | Sports | 体育 |

## Subscribe to Daily News

When the user wants daily news delivery (e.g. "/news subscribe politics,tech", "订阅新闻"):

1. Use `cron` tool to create a recurring job:
```json
{"action": "add", "name": "daily-news", "message": "/news politics,tech", "cron_expr": "0 0 9 * * * *"}
```

2. Adjust the cron time based on user preference (default: 9:00 AM daily).

## Unsubscribe

When the user says "/news unsubscribe" or "取消新闻订阅":

1. Use `cron` tool to list jobs: `{"action": "list"}`
2. Find the job named `daily-news` and remove it: `{"action": "remove", "job_id": "..."}`

## Language

Default output language is Chinese (zh). If the user asks for English, pass `"language": "en"`.

## Notes

- The tool uses a hybrid approach: RSS/API for headline discovery, then deep-fetches top articles for full content
- Sources: Google News RSS, Hacker News API, Yahoo News HTML, Substack RSS, Medium RSS
- Results are saved to `~/.crew/research/news-digest-{date}.md`
- When triggered by cron, call `news_digest` with the categories from the cron message
