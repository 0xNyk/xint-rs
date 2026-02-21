<!-- markdownlint-disable MD041 -->
<p align="center">
  <strong>xint-rs</strong> — X Intelligence CLI
</p>

<p align="center">
  <strong>Single binary, zero runtime dependencies.</strong> 2.5MB that starts in under 5ms.
</p>

<p align="center">
  <a href="LICENSE"><img src="https://img.shields.io/badge/License-MIT-blue.svg" alt="License: MIT"></a>
  <a href="https://www.rust-lang.org"><img src="https://img.shields.io/badge/Built_with-Rust-dea584.svg" alt="Rust"></a>
  <a href="https://github.com/0xNyk/xint-rs/releases"><img src="https://img.shields.io/github/v/release/0xNyk/xint-rs?display_name=tag" alt="Release"></a>
  <a href="https://github.com/0xNyk/xint-rs/stargazers"><img src="https://img.shields.io/github/stars/0xNyk/xint-rs" alt="Stars"></a>
  <a href="https://twitter.com/intent/tweet?text=Check+out+xint-rs:+Fast+X+Intelligence+CLI+in+Rust+%F0%9F%98%8E%0Ahttps://github.com/0xNyk/xint-rs"><img src="https://img.shields.io/twitter/url?label=Tweet&url=https%3A%2F%2Fgithub.com%2F0xNyk%2Fxint-rs" alt="Tweet"></a>
</p>

---

> **Search X like a pro.** Full-text search, real-time monitoring, follower tracking, AI analysis — all from CLI. Built in Rust for speed.

## Why Rust?

| | TypeScript | Rust |
|---|---|---|
| **Startup** | ~50ms | <5ms |
| **Binary** | ~60MB | 2.5MB |
| **Memory** | ~40MB | ~5MB |
| **Deploy** | Clone + Bun | Copy one file |

## Install

```bash
curl -fsSL https://raw.githubusercontent.com/0xNyk/xint-rs/main/install.sh | bash
```

Optional pinned version:

```bash
XINT_RS_INSTALL_VERSION=<version-tag> \
curl -fsSL https://raw.githubusercontent.com/0xNyk/xint-rs/main/install.sh | bash
```

Homebrew (lightweight prebuilt binary on Apple Silicon):

```bash
brew tap 0xNyk/xint
brew install xint
```

Or install the explicit Rust formula:

```bash
brew install xint-rs
```

Or build from source:

```bash
git clone https://github.com/0xNyk/xint-rs.git
cd xint-rs
cargo build --release
```

> **Requires:** [X API access](https://developer.x.com) (prepaid credits)

## Quick Reference

| Task | Command |
|------|---------|
| Search | `xint search "AI agents"` |
| Monitor | `xint watch "solana" -i 5m` |
| Stream | `xint stream` |
| Profile | `xint profile @elonmusk` |
| Thread | `xint thread 123456789` |
| Followers | `xint diff @username` |
| Bookmarks | `xint bookmarks` |
| Lists | `xint lists` |
| Blocks | `xint blocks` |
| Mutes | `xint mutes` |
| Follow | `xint follow @username` |
| Media | `xint media <tweet_id>` |
| Trends | `xint trends` |
| AI Analyze | `xint analyze "best?"` |
| Report | `xint report "crypto"` |
| Article | `xint article <url> --ai "summarize"` |
| Capabilities | `xint capabilities --json` |
| TUI | `xint tui` |

### Shorthands

```bash
xint s "query"    # search
xint w "query"    # watch
xint p @user     # profile
xint tr           # trends
xint bm           # bookmarks
```

### TUI Customization

```bash
# Built-in themes: classic | neon | minimal | ocean | amber
XINT_TUI_THEME=ocean xint tui

# Disable animated hero line
XINT_TUI_HERO=0 xint tui

# Disable icons in menu rows
XINT_TUI_ICONS=0 xint tui

# Optional theme token file
XINT_TUI_THEME_FILE=./tui-theme.tokens.example.json xint tui
```

## Setup

```bash
cp .env.example .env
# Add X_BEARER_TOKEN=your_token
```

### Optional: xAI

For `analyze`, `report --sentiment`, `article --ai`:

```bash
XAI_API_KEY=your_xai_key
```

### Optional: OAuth

For bookmarks, likes, follows, lists, blocks/mutes, follower tracking:

```bash
X_CLIENT_ID=your_client_id
xint auth setup
```

## Deployment Modes

### Self-hosted (OSS default)

- Run this binary against your own local setup.
- Package API features stay local unless cloud env vars are configured.
- Best for development and private operator workflows.

### Hosted cloud control plane (`xint-cloud`)

- Point package API features to your hosted control plane:
  - `XINT_PACKAGE_API_BASE_URL=http://localhost:8787/v1` (or your deployed URL)
  - `XINT_PACKAGE_API_KEY=<workspace_api_key>`
  - `XINT_WORKSPACE_ID=<workspace_id>`
- Optional billing upgrade URL for quota/plan errors:
  - `XINT_BILLING_UPGRADE_URL=https://your-app/pricing`

Notes:
- If `XINT_PACKAGE_API_BASE_URL` is unset, package API MCP tools return a setup error.
- Keep `xint-cloud` private; `xint` and `xint-rs` remain public OSS clients.

## Agent-Native Capabilities Manifest

`xint-rs` ships a machine-readable manifest for agent allowlists and runtime tool selection:

```bash
# Pretty JSON
xint capabilities

# Compact JSON for machine ingestion
xint capabilities --compact
```

## Search

```bash
# Quick pulse
xint search "AI agents" --quick

# High-engagement
xint search "react 19" --since 1h --sort likes --min-likes 50

# Full-archive
xint search "bitcoin ETF" --full --pages 3

# With sentiment
xint search "solana" --sentiment

# Export
xint search "startups" --csv > data.csv
xint search "AI" --jsonl | jq '.text'
```

### Options

| Flag | Description |
|------|-------------|
| `--sort` | `likes` · `impressions` · `retweets` · `recent` |
| `--since` | `1h` · `3h` · `12h` · `1d` · `7d` |
| `--full` | Search full archive (back to 2006) |
| `--sentiment` | AI sentiment per tweet |
| `--quick` | Fast mode with caching |

## Watch

```bash
xint watch "solana" -i 5m
xint watch "@user" -i 1m
xint watch "news" -i 30s --webhook https://example.com/webhook
```

Webhook safety:
- Remote webhooks must use `https://`
- `http://` is accepted only for localhost/loopback targets
- Optional host allowlist: `XINT_WEBHOOK_ALLOWED_HOSTS=hooks.example.com,*.internal.example`

Press `Ctrl+C` — shows session stats.

## Stream (Official Filtered Stream)

```bash
# List current stream rules
xint stream-rules

# Add a filtered-stream rule
xint stream-rules add "from:elonmusk -is:retweet" --tag elon

# Connect to stream
xint stream

# JSONL output and stop after 25 events
xint stream --jsonl --max-events 25
```

## Follower Tracking

```bash
xint diff @user        # First run: snapshot
xint diff @user        # Second run: changes
xint diff @user --following
```

Requires OAuth.

## Lists (OAuth)

```bash
xint lists
xint lists create "AI Researchers" --description "High-signal accounts" --private
xint lists members add <list_id> @username
xint lists members remove <list_id> @username
```

## Blocks & Mutes (OAuth)

```bash
xint blocks
xint blocks add @username
xint blocks remove @username

xint mutes
xint mutes add @username
xint mutes remove @username
```

## Follow Actions (OAuth)

```bash
xint follow @username
xint unfollow @username
```

## Media Download

```bash
# Download media from a tweet ID
xint media 1900100012345678901

# Download media from a tweet URL
xint media https://x.com/user/status/1900100012345678901

# Custom output directory + JSON summary
xint media 1900100012345678901 --dir ./downloads --json

# Download only first video/gif
xint media 1900100012345678901 --video-only --max-items 1

# Download only photos
xint media 1900100012345678901 --photos-only

# Custom filename template
xint media 1900100012345678901 --name-template "{username}-{created_at}-{index}"
```

## Reports & Analysis

```bash
xint report "AI agents" --save
xint analyze "What's trending in crypto?"
xint article "https://..." --ai "Summarize"

# From X tweet (auto-extract linked article URL)
xint article "https://x.com/user/status/123" --ai "Summarize"
```

## xAI Features

### X Search (no X API needed)

```bash
xint x-search --queries-file queries.json --out-md report.md
```

### Collections (Knowledge Base)

```bash
xint collections list
xint collections upload --path file.md
xint collections search --query "topic"
```

## AI Agent Skill

Designed for Claude Code, OpenClaw, and other agents:

```bash
# Place binary + SKILL.md in agent skills dir
xint search "topic" --quick --json
xint analyze --pipe "Summarize"
xint report "topic" --save
```

### MCP Server

```bash
xint mcp
```

## Cost

| Operation | Cost |
|-----------|------|
| Tweet read | $0.005/tweet |
| Full-archive | $0.01/tweet |
| Write | $0.01/action |

```bash
xint costs           # Today
xint costs week      # 7 days
xint costs budget 2  # Set $2/day limit
```

## Environment

| Variable | Required | Description |
|----------|----------|-------------|
| `X_BEARER_TOKEN` | Yes | X API v2 bearer |
| `XAI_API_KEY` | No | xAI for analyze/report |
| `XINT_ARTICLE_TIMEOUT_SEC` | No | Article fetch timeout seconds (default 30, range 5-120) |
| `X_CLIENT_ID` | No | OAuth for write ops |

## Structure

```
xint-rs/
├── src/
│   ├── main.rs           # Entry
│   ├── cli.rs           # Commands
│   ├── client.rs        # HTTP + rate limit
│   ├── api/             # X, xAI wrappers
│   └── commands/         # 20+ commands
├── data/                 # cache, exports, snapshots
└── SKILL.md             # Agent instructions
```

## Build

```bash
cargo build --release
# Output: target/release/xint (2.5MB)
```

## Release Automation

`xint-rs` delegates releases to the canonical script in `xint`.

```bash
# from xint-rs/
./scripts/release.sh --dry-run --allow-dirty
# forwards all flags to the canonical xint script:
./scripts/release.sh 2026.2.18.4
./scripts/release.sh 2026.2.18.4 --no-clawdhub
./scripts/release.sh 2026.2.18.4 --skillsh
./scripts/release.sh 2026.2.18.4 --no-auto-notes
./scripts/release.sh 2026.2.18.4 --report-dir /tmp/xint-release-reports
```

If `xint` is not checked out as a sibling directory, set:

```bash
XINT_RELEASE_SCRIPT=/absolute/path/to/xint/scripts/release.sh
```

Notes behavior is controlled by the canonical script:

- Default: GitHub auto-generated notes (`--generate-notes`)
- Manual override: set `CHANGELOG_ADDED`, `CHANGELOG_CHANGED`, `CHANGELOG_FIXED`, and/or `CHANGELOG_SECURITY`
- Default: publishes to ClawdHub when `clawdhub` CLI is installed (disable with `--no-clawdhub`)
- Optional: publish to skills.sh with `--skillsh` (or `--ai-skill` for both)
- Release report: `reports/releases/<version>.md` by default (disable with `--no-report`)
- Report is uploaded to both GitHub releases as an asset by default (disable with `--no-report-asset`)
- Report is embedded in both GitHub release bodies by default (disable with `--no-report-body`)

## Security

- Tokens from env — never hardcoded
- OAuth tokens: `chmod 600`
- No telemetry, no phone-home

See [SECURITY.md](docs/security.md).

## License

[MIT](LICENSE) · [0xNyk](https://github.com/0xNyk)

---

<p align="center">
  <a href="https://star-history.com/#0xNyk/xint-rs&Date">
    <img src="https://api.star-history.com/svg?repos=0xNyk/xint-rs&type=Date" alt="Star History" width="400">
  </a>
</p>
