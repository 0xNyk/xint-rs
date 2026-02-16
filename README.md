# xint-rs

<p align="center">
  <strong>X Intelligence CLI</strong> — search, monitor, analyze, and engage on X/Twitter. Single binary, zero runtime dependencies.
</p>

<p align="center">
  <a href="LICENSE"><img src="https://img.shields.io/badge/License-MIT-blue.svg" alt="License: MIT"></a>
  <a href="https://www.rust-lang.org"><img src="https://img.shields.io/badge/Built_with-Rust-dea584.svg" alt="Built with Rust"></a>
  <img src="https://img.shields.io/badge/Binary-2.5MB-green.svg" alt="Binary: 2.5MB">
</p>

---

Rust rewrite of [xint](https://github.com/0xNyk/xint). Same 20+ commands, same API coverage — compiled to a **2.5MB static binary** that starts in under 5ms. No Node.js, no Bun, no runtime.

Built for **AI agents** that invoke xint hundreds of times per session — every millisecond of startup overhead compounds. Also works standalone for researchers, OSINT practitioners, and power users.

## Why Rust?

| | TypeScript (xint) | Rust (xint-rs) |
|---|---|---|
| **Startup** | ~50ms (Bun runtime) | <5ms |
| **Binary size** | ~60MB (with Bun) | 2.5MB |
| **Dependencies** | Bun + npm packages | Single static binary |
| **Memory** | ~40MB baseline | ~5MB baseline |
| **Deploy** | Clone repo + install Bun | Copy one file |

For agents running search → analyze → report loops, the Rust version saves **~150ms per cycle** in startup alone.

## Quick Start

### From source

```bash
git clone https://github.com/0xNyk/xint-rs.git
cd xint-rs
cp .env.example .env
# Edit .env — add your X_BEARER_TOKEN

cargo build --release
./target/release/xint search "your topic" --limit 10
```

### From release binary

```bash
# Download the latest release binary for your platform
# Place it somewhere in your PATH
chmod +x xint
xint search "your topic" --limit 10
```

## Configuration

Set in `.env` or as environment variables:

| Variable | Required | Description |
|----------|----------|-------------|
| `X_BEARER_TOKEN` | Yes | X API v2 bearer token ([get one here](https://developer.x.com)) |
| `X_CLIENT_ID` | For OAuth | OAuth 2.0 client ID (bookmarks, likes, following, diff) |
| `XAI_API_KEY` | For Grok | xAI API key (analyze, sentiment, reports, x-search, collections) |
| `XAI_MANAGEMENT_API_KEY` | For Collections | xAI Management API key (collections management) |

## Commands

| Command | Alias | Auth | Description |
|---------|-------|------|-------------|
| `search <query>` | `s` | Bearer | Search tweets with sorting, filtering, export |
| `watch <query>` | `w` | Bearer | Real-time monitoring with polling + webhooks |
| `diff <@user>` | `followers` | OAuth | Track follower/following changes over time |
| `report <topic>` | — | Bearer + xAI | Intelligence report with AI summary |
| `thread <id>` | `t` | Bearer | Fetch full conversation thread |
| `profile <user>` | `p` | Bearer | User profile + recent tweets |
| `tweet <id>` | — | Bearer | Fetch a single tweet |
| `article <url>` | `read` | xAI | Fetch & analyze article content |
| `bookmarks` | `bm` | OAuth | List bookmarked tweets |
| `bookmark <id>` | — | OAuth | Bookmark a tweet |
| `unbookmark <id>` | — | OAuth | Remove a bookmark |
| `likes` | — | OAuth | List liked tweets |
| `like <id>` | — | OAuth | Like a tweet |
| `unlike <id>` | — | OAuth | Unlike a tweet |
| `following [user]` | — | OAuth | List accounts you follow |
| `trends [location]` | `tr` | Bearer | Trending topics (30+ countries) |
| `analyze <query>` | `ask` | xAI | AI analysis via Grok |
| `costs [period]` | `cost` | — | API cost tracking + budgets |
| `watchlist [cmd]` | `wl` | — | Account watchlist management |
| `auth [cmd]` | — | — | OAuth setup / status / refresh |
| `cache [cmd]` | — | — | Cache management |
| `x-search` | `xs` | xAI | xAI-hosted X search (Responses API, no cookies) |
| `collections [cmd]` | `col` | xAI + Mgmt | Knowledge base: upload, search, sync files |
| `mcp` | `mcp-server` | — | Start MCP server for AI agents |

## Usage

### Search & Discovery

```bash
# Quick pulse check (1 page, 10 results, 1hr cache)
xint search "AI agents" --quick

# High-engagement tweets sorted by likes
xint search "react 19" --since 1h --sort likes --min-likes 50

# Full-archive search (back to 2006)
xint search "bitcoin ETF" --full --pages 3 --save

# With AI sentiment analysis
xint search "solana" --sentiment --limit 20

# Export formats
xint search "startup funding" --csv > funding.csv
xint search "AI" --jsonl | jq 'select(.metrics.likes > 100)'
xint search "topic" --json
xint search "topic" --markdown --save
```

### Real-Time Monitoring

```bash
# Watch a topic every 5 minutes
xint watch "solana memecoins" -i 5m

# Watch a specific user (auto-expands @user to from:user)
xint watch "@vitalikbuterin" -i 1m

# Webhook integration (Slack, Discord, n8n, etc.)
xint watch "breaking news" -i 30s --webhook https://hooks.slack.com/...

# Machine-readable output
xint watch "topic" --jsonl | tee -a feed.jsonl
```

Press `Ctrl+C` to stop — prints session stats (duration, tweets found, estimated cost).

### Intelligence Reports

```bash
# Full report with AI summary
xint report "AI agents"

# Track specific accounts + sentiment
xint report "solana" -s -a @aeyakovenko,@rajgokal --save

# Stronger model for deeper analysis
xint report "crypto market" --model grok-3 -s --save
```

Reports include: executive summary, sentiment table, top tweets by engagement, per-account activity, and metadata.

### Follower Tracking

```bash
# First run creates a snapshot, subsequent runs show diff
xint diff @username

# Track following changes instead
xint diff @username --following

# View snapshot history
xint diff @username --history
```

Requires OAuth (`xint auth setup`). Snapshots stored locally in `data/snapshots/`.

### Grok AI Analysis

```bash
# Direct question
xint analyze "What are the top AI agent frameworks?"

# Analyze tweets from file
xint analyze --tweets saved.json

# Pipe from search
xint search "AI" --json | xint analyze --pipe "Summarize the key themes"

# Custom system prompt
xint analyze --system "You are a crypto analyst" "What's moving today?"
```

### Article Fetching & Analysis

Fetch and extract full article content from any URL using xAI's web_search tool. Also supports extracting linked articles from X tweets.

```bash
# Fetch article content
xint article "https://example.com"

# Fetch + analyze with AI
xint article "https://example.com" --ai "Summarize key takeaways"

# Auto-extract article from X tweet URL and analyze
xint article "https://x.com/user/status/123456789" --ai "What are the main points?"

# Full content without truncation
xint article "https://example.com" --full

# JSON output
xint article "https://example.com" --json
```

The `article` command:
- Uses xAI's `grok-4` model with web_search tool (requires `XAI_API_KEY`)
- Extracts title, author, publication date, word count, reading time
- `--ai` flag passes article content to Grok for analysis
- Auto-detects X tweet URLs and extracts linked articles

### Trends

```bash
xint trends              # Worldwide
xint trends us           # United States
xint trends japan        # Japan
xint trends --json       # JSON output
xint trends --locations  # List all 30+ supported locations
```

### Bookmarks & Engagement

```bash
xint bookmarks                    # List bookmarks
xint bookmarks --since 1d        # Recent only
xint bookmark 1234567890          # Save a tweet
xint unbookmark 1234567890        # Remove

xint likes                        # List liked tweets
xint like 1234567890              # Like
xint unlike 1234567890            # Unlike

xint following                    # Who you follow
```

### Cost Tracking

```bash
xint costs                # Today's spending
xint costs week           # Last 7 days
xint costs month          # Last 30 days
xint costs budget 2.00    # Set $2/day limit
xint costs reset          # Reset today's tracking
```

Default budget: $1.00/day. The `watch` command auto-stops when the budget is exceeded.

### Watchlist

```bash
xint watchlist                          # List all
xint watchlist add @user "competitor"   # Add with note
xint watchlist remove @user             # Remove
xint watchlist check @user              # Check membership
```

### xAI X Search

Search X via xAI's hosted `x_search` tool — no cookies, no GraphQL, no X API bearer token needed. Uses the Responses API.

```bash
# Search with queries from a JSON file
echo '["AI agents", "solana DeFi"]' > queries.json
xint x-search --queries-file queries.json --out-md report.md --out-json raw.json

# Custom model and date range
xint x-search --queries-file queries.json --model grok-3 --from-date 2026-02-01

# With memory candidate emission (for agent workflows)
xint x-search --queries-file queries.json --workspace /path/to/workspace --emit-candidates
```

### xAI Collections (Knowledge Base)

Upload files, manage collections, and semantic-search your documents via xAI's Files + Collections APIs.

```bash
# List collections
xint collections list

# Create or ensure a collection exists
xint collections ensure --name "research-kb"

# Upload a file
xint collections upload --path report.md

# Search across collections
xint collections search --query "AI agent frameworks" --collection-ids id1,id2

# Sync a directory of files to a collection
xint collections sync-dir --collection-name "kb" --dir ./docs --glob "*.md" --limit 50
```

Requires `XAI_API_KEY` (file upload + search) and `XAI_MANAGEMENT_API_KEY` (collections management).

## OAuth Setup

Bookmarks, likes, following, and follower tracking require OAuth 2.0 PKCE:

1. [X Developer Portal](https://developer.x.com) → Your App → Settings
2. Enable **OAuth 2.0** with **Public client** type
3. Add callback URL: `http://127.0.0.1:3333/callback`
4. Set `X_CLIENT_ID` in `.env`
5. Run:

```bash
xint auth setup           # Opens browser, captures callback
xint auth setup --manual  # Headless: paste redirect URL manually
xint auth status          # Check token info
xint auth refresh         # Force refresh
```

Tokens stored in `data/oauth-tokens.json` (chmod 600), auto-refresh on expiry.

## AI Agent Skill

xint is designed as a skill for AI agents. The [`SKILL.md`](SKILL.md) file provides structured instructions for autonomous X intelligence operations.

### Claude Code / OpenClaw

Place the binary and `SKILL.md` in your agent's skill directory. The agent reads `SKILL.md` and runs commands like:

```bash
xint search "topic" --quick --json
xint analyze --pipe "Summarize sentiment"
xint report "topic" --save
```

### Agent advantages of the Rust binary

- **<5ms startup** — no runtime initialization overhead
- **Single file deploy** — `scp xint user@server:/usr/local/bin/`
- **Low memory** — ~5MB vs ~40MB for the Bun version
- **Predictable performance** — no GC pauses during long `watch` sessions

## Project Structure

```
xint-rs/
├── Cargo.toml
├── .env.example
├── SKILL.md                  Agent instructions
├── src/
│   ├── main.rs               Entry point + command dispatch
│   ├── cli.rs                Clap derive (all 20 commands)
│   ├── client.rs             Shared reqwest client + rate limiting
│   ├── config.rs             Env loading + path resolution
│   ├── models.rs             Tweet, User, Trend, Snapshot, etc.
│   ├── cache.rs              Generic file cache (MD5 keys, TTL)
│   ├── costs.rs              Cost tracking + budget system
│   ├── format.rs             Terminal, JSON, JSONL, CSV, Markdown
│   ├── sentiment.rs          Batched sentiment via Grok
│   ├── api/
│   │   ├── twitter.rs        X API v2 (search, tweet, profile, thread)
│   │   ├── grok.rs           xAI Grok (chat completions)
│   │   └── xai.rs            xAI Responses API + Collections/Files
│   ├── auth/
│   │   └── oauth.rs          OAuth 2.0 PKCE (callback server, refresh)
│   └── commands/
│       ├── search.rs          search, with cache + sentiment + export
│       ├── watch.rs           Real-time polling loop
│       ├── diff.rs            Follower snapshots + diffs
│       ├── report.rs          Intelligence reports
│       ├── trends.rs          Trending topics (API + fallback)
│       ├── analyze.rs         Grok AI analysis
│       ├── bookmarks.rs       Bookmarks list + filter
│       ├── engagement.rs      Likes, like, unlike, bookmark, following
│       ├── thread.rs          Thread fetch
│       ├── profile.rs         User profile
│       ├── tweet.rs           Single tweet
│       ├── costs_cmd.rs       Cost tracking CLI
│       ├── watchlist.rs       Watchlist CRUD
│       ├── auth_cmd.rs        Auth setup / status / refresh
│       ├── cache_cmd.rs       Cache clear / status
│       ├── x_search.rs        xAI X Search (Responses API)
│       └── collections.rs     xAI Collections KB management
└── data/                     Runtime data (gitignored)
    ├── cache/
    ├── exports/
    └── snapshots/
```

## Building

```bash
# Debug build
cargo build

# Release build (optimized, stripped, 2.5MB)
cargo build --release

# Cross-compile for Linux x86_64
cargo build --release --target x86_64-unknown-linux-gnu
```

## Security

- Bearer tokens read from env vars / `.env` — never printed to stdout
- OAuth tokens stored with `chmod 600` + atomic writes (write tmp → rename)
- PKCE with SHA-256 code challenge — no client secret needed
- Follower snapshots stored locally, never transmitted
- No telemetry, no analytics, no phone-home

## Cost Reference

X API v2 pay-per-use rates:

| Resource | Cost |
|----------|------|
| Tweet read (search, bookmarks, likes) | ~$0.005/tweet |
| Full-archive tweet read | ~$0.01/tweet |
| Write operations (like, bookmark) | ~$0.01/action |
| Trends request | ~$0.10/request |

Use `--quick` mode and caching to minimize costs. Budget system prevents runaway spending.

## License

[MIT](LICENSE)
