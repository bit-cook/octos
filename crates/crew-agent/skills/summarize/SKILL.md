---
name: summarize
description: Summarize or extract text from URLs, podcasts, and local files.
requires_bins: summarize
---

# Summarize

Use the `summarize` CLI to extract and summarize content from URLs and files.

## Usage

```bash
# Summarize a URL
summarize https://example.com/article

# Summarize a YouTube video (extracts transcript)
summarize https://youtube.com/watch?v=...

# Summarize a local file
summarize document.pdf

# With custom model
summarize --model gpt-4o https://example.com/article
```

## Trigger Phrases

Use this skill when the user asks to:
- "Summarize this URL/article/page"
- "What does this link say?"
- "Transcribe this YouTube video"
- "Extract text from this PDF"

## Installation

```bash
# macOS
brew install steipete/tap/summarize

# Or via cargo
cargo install summarize
```

## Tips

- Works with most web pages, PDFs, and video URLs
- YouTube transcription uses auto-generated captions when available
- Set `OPENAI_API_KEY` or `ANTHROPIC_API_KEY` for summarization model
