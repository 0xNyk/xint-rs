# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added — Forecast (2026-05-16)
- **`xint costs forecast`** — mirrors xint TS. Projects end-of-month spend from MTD burn rate (or 7-day trailing average when MTD < 3 days). Includes top-3 expensive operations, share-of-spend, and confidence note. `--json` for structured output. New `forecast_month()` in `costs.rs` + `MonthForecast` / `ForecastOpShare` structs.

### Added — Cost preview (2026-05-16)
- **`--dry-run` flag** on `search` and `diff` — prints estimated cost preview + cache-hit prediction + endpoint, then exits 0 without any API call. New `dryrun.rs` module mirrors `xint/lib/dryrun.ts`.

### Fixed
- **Field-profile recalibration** — Mirror of TS fix. MINIMAL_FIELDS now keeps entities + conversation_id (parsed on every tweet read). STANDARD adds connection_status. EXTENDED adds article + note_tweet + subscription_type. `get_tweet` uses EXTENDED_FIELDS.
- **Followers cost rate** — `costs.rs` had `followers` at `(0.0, 0.01)` (per_call); X API charges per user. Changed to `(0.01, 0.0)`. A 3000-follower diff now correctly previews at $30.

### Added — X API efficiency pass (2026-05-16)
- **Field profiles in `client.rs`** — `MINIMAL_FIELDS` / `STANDARD_FIELDS` / `EXTENDED_FIELDS` constants + `FieldLevel` enum + `fields_for()` helper. `FIELDS` is now an alias for `MINIMAL_FIELDS` so existing call sites get the cheap default. `SearchArgs` exposes `--full-fields` for opting into extended payloads.
- **Followers snapshot cache (24h TTL)** — `commands/diff.rs` reuses the most recent snapshot when <24h old. `DiffArgs::fresh` (`--fresh`) bypasses the cache. Cuts the ~$50 cost of a 5000-follower diff to $0 on same-day repeats.
- **Watch seed-window narrowing** — `WatchArgs::since` default changed from `1h` to `10m`. `--seed-window` is now an alias for `--since`.
- **Archive search confirmation gate** — `SearchArgs` adds `--confirm`. `--full` / `--full-archive` without `--confirm` prints estimated max cost and exits 2.

### Added — Grok credit-aware agent mode (2026-05)
- **`credits` command** — Grok free-tier telemetry mirroring xint TS. Shows monthly burn against console.x.ai's $25 signup + $150/mo data-share. Subcommands: `--setup`, `--data-sharing on|off`. Triggered automatically by `analyze` when `XAI_API_KEY` is missing (exits 0 so agents can read the guide).
- **`--budget` flag on `analyze`** — `cheap` (default, grok-4-1-fast) | `balanced` (grok-4.3) | `max` (grok-4.20-reasoning). Image inputs auto-route to grok-4.3.
- **`cost_in_usd_ticks` tracking** — Authoritative cost from xAI's response `usage` is used when present; local pricing table is the fallback.
- **`Config::try_xai_key()`** — Non-throwing variant of `require_xai_key()` so commands can present the credit guide instead of failing.
- **Premium chat-routing tip** — Mirrors xint TS. Opt-in via `XINT_X_PREMIUM=Premium|Premium+|PremiumPlus` and `XINT_PREMIUM_TIPS=1`. For one-shot human queries, suggests pasting into https://grok.com to spend Premium chat allowance instead of API credits. Prints to stderr so JSON output stays clean.

### Added
- **`reposts` command** — Look up who reposted a tweet (`GET /2/tweets/:id/retweeted_by`). Supports `--limit`, `--json` output.
- **`users` command** — Search users by keyword (`GET /2/users/search`). Supports `--limit`, `--json` output.
- **`connection_status` field** — Profile output now shows follow/block/mute relationship status.
- **`subscription_type` field** — Profile output now shows Premium/verified badge.
- **`--vision` flag** on `x-search` — Enables `enable_image_understanding` for image analysis during search.
- **`--exclude-domains` / `--allow-domains` flags** on `x-search` — Domain filtering via xAI search tools.
- **Inline citations** — `x-search` output now includes source citations from xAI Responses API.
- **403 tier handling** — Graceful error messages when blocks/follows endpoints are restricted by API tier.

### Changed — Grok credit-aware agent mode (2026-05)
- **Grok model pricing refreshed (2026-05 retirement window)** — Added `grok-4.3` ($1.25/$2.50, 1M ctx). Pricing patterns now match `grok-4.20`, `grok-4.20-reasoning`, `grok-4.20-non-reasoning`, all `grok-4-1-fast*` variants. Retired-but-redirected models kept for graceful lookup post-2026-05-15.
- **Vision default switched to `grok-4.3`** — `grok-2-vision` retires 2026-05-15; we move explicitly rather than rely on xAI's auto-redirect.
- **402 error hint** — Points to `xint credits` and console.x.ai instead of generic message.

### Changed
- **Default Grok model** — Switched from `grok-3-mini` to `grok-4-1-fast` across analyze, report, sentiment, and MCP tools ($0.20/$0.50 per 1M tokens — better price/performance).
- **xAI cost rates reduced** — Updated cost tracking to reflect ~50% price drop on xAI tool invocations.
- **Model pricing map** — Added grok-4 ($3.00/$15.00), grok-4-1-fast ($0.20/$0.50), grok-4-1-fast-reasoning ($0.20/$0.50).
- **MCP tool descriptions** — Updated model references in all MCP tool schemas.
- **SKILL.md** — Updated model defaults, cost table, added new commands.
- **README.md** — Added reposts/users commands, updated model reference.

## [3.1.0] (2026-02-15) — Agent Intelligence Update

Major feature release focused on real-time intelligence, social graph tracking, AI-powered sentiment, and structured export formats. Designed to make xint the most capable X intelligence skill for AI agents.

### ✨ Added
- **`watch` command** — Real-time monitoring. Polls a search query on interval, shows only new tweets. Supports webhook POST (Slack, Discord, etc.), JSONL output for piping, graceful shutdown with session stats. Auto-handles rate limits.
- **`diff` command** — Follower/following tracking with local snapshots. Shows who followed/unfollowed since last check. Supports `--following` for tracking who you follow, `--history` to view all snapshots, `--json` for structured output.
- **`report` command** — Automated intelligence reports combining search + Grok AI analysis + optional sentiment. Generates markdown with executive summary, top tweets, per-account activity, and metadata. `--save` writes to `data/exports/`.
- **`--sentiment` flag** — AI-powered per-tweet sentiment analysis on search results via Grok. Shows positive/negative/neutral/mixed with scores (-1.0 to 1.0) and aggregate stats. Uses batched Grok calls with structured JSON parsing.
- **`--csv` flag** — CSV output for spreadsheet analysis. Proper escaping, header row, all tweet fields.
- **`--jsonl` flag** — One JSON object per line. Optimized for Unix pipelines: `xint search "topic" --jsonl | jq 'select(.metrics.likes > 100)'`
- **`data/snapshots/` directory** — Local storage for follower/following snapshots used by `diff` command.

### 🔧 Changed
- **README rewritten** — Hero image, agent-first positioning, feature table, all new commands documented, "Use as an AI Agent Skill" section expanded.
- **Commands table expanded** — 25 commands total (was 21), with shortcuts for `watch` (`w`), `diff` (also `followers`).
- **Usage text updated** — All new commands and flags documented in `--help`.
- **Cost tracking** — Added `followers` and `following_list` cost rates.

---

## [3.0.0] (2026-02-15) — xint

**Rebranded as xint (X Intelligence).** Open-sourced under MIT license at [github.com/0xNyk/xint](https://github.com/0xNyk/xint).

### Added
- **OAuth 2.0 PKCE authentication** — `auth setup`, `auth status`, `auth refresh` commands for user-context operations
- **Bookmarks** — `bookmarks`, `bookmark`, `unbookmark` commands (read + write)
- **Likes** — `likes`, `like`, `unlike` commands (read + write)
- **Following** — `following [username]` to list accounts you follow
- **Trending topics** — `trends [location]` with 30+ countries, API + search fallback
- **Grok AI analysis** — `analyze` command powered by xAI (grok-3, grok-3-mini, grok-2)
- **Cost management** — per-call tracking, daily budgets, weekly/monthly reports
- **Full-archive search** — `--full` flag for searching back to 2006
- **`package.json`**, **`tsconfig.json`**, **`.env.example`** — proper project scaffolding
- **`CONTRIBUTING.md`** — contribution guide

### Changed
- **Renamed** `x-search.ts` to `xint.ts`, `x-research` to `xint` throughout
- **Generalized env paths** — no more hardcoded server paths; reads from `.env` at project root
- **Save location** — `--save` now writes to `data/exports/` (was `data/exports/`)
- **Community-ready README** — badges, command table, OAuth guide, Grok docs, cost reference

### Removed
- Hardcoded paths to `/home/openclaw/`, `~/.config/env/global.env`, `~/.openclaw/.env`
- Personal usernames and server-specific references

---

## [2.3.0] (2026-02-09)

### 🔒 Security
- **Purged all stale tier/subscription references** across 6 files (13 instances of "Basic tier", "current tier", "enterprise-only" etc.) — LLM hallucination fix
- **Security section in README** — Documents bearer token exposure risk when running inside AI coding agents with session logging

### 🐛 Fixed
- **Tweet truncation bug** — `tweet` and `thread` commands now show full tweet text instead of cutting off at 200 characters. Search results still truncate for readability. (h/t @sergeykarayev)

### ✨ Added
- **Full-archive search** (`/2/tweets/search/all`) is available on pay-per-use — not enterprise-only as LLMs commonly claim
- **Updated rate limits** — old per-15-min caps replaced by spending limits in Developer Console
- **Clarified 7-day limit** — is a skill limitation (using recent search endpoint), not an API restriction
- **Query length limits** — 512 chars (recent), 1024 (full-archive), 4096 (enterprise)
- **Per-resource cost breakdown** — $0.005/post read, $0.010/user lookup, $0.010/post create
- **24-hour deduplication** docs, xAI credit bonus tiers, usage monitoring endpoint

---

## [2.2.0] (2026-02-08)

### ✨ Added
- **`--quick` mode** — Smarter, cheaper searches. Single page, auto noise filtering (`-is:retweet -is:reply`), 1hr cache TTL. Designed for fast pulse checks.
- **`--from <username>`** — Shorthand for `from:username` queries. `search "topic" --from username` instead of typing the full operator.
- **`--quality` flag** — Filters out low-engagement tweets (≥10 likes). Applied post-fetch since `min_faves` operator isn't available via the API.
- **Cost display on all searches** — Every search now shows estimated API cost: `📊 N tweets read · est. cost ~$X`

### 🔧 Changed
- README cleaned up — removed duplicate cost section, added Quick Mode and Cost docs
- Cache supports variable TTL (1hr in quick mode, 15min default)

---

## [2.1.0] (2026-02-08)

### ✨ Added
- **`--since` time filter** — search only recent tweets: `--since 1h`, `--since 3h`, `--since 30m`, `--since 1d`
  - Accepts shorthand (`1h`, `30m`, `2d`) or ISO 8601 timestamps
  - Great for monitoring during catalysts or checking what just dropped
- Minutes support (`30m`, `15m`) in addition to hours and days
- Cache keys now include time filter to prevent stale results across different time ranges

---

## [2.0.0] (2026-02-08)

### ✨ Added
- **`x-search.ts` CLI** — Bun script wrapping the X API. No more inline curl/python one-liners.
  - `search` — query with auto noise filtering, engagement sorting, pagination
  - `profile` — recent tweets from any user
  - `thread` — full conversation thread by tweet ID
  - `tweet` — single tweet lookup
  - `watchlist` — manage accounts to monitor, batch-check recent activity
  - `cache clear` — manage result cache
- **`lib/api.ts`** — Typed X API wrapper with search, thread, profile, tweet lookup, engagement filtering, deduplication
- **`lib/cache.ts`** — File-based cache with 15-minute TTL. Avoids re-fetching identical queries.
- **`lib/format.ts`** — Output formatters for Telegram (mobile-friendly) and markdown (research docs)
- **Watchlist system** — `data/watchlist.json` for monitoring accounts. Useful for heartbeat integration.
- **Auto noise filtering** — `-is:retweet` added by default unless already in query
- **Engagement sorting** — `--sort likes|impressions|retweets|recent`
- **Post-hoc filtering** — `--min-likes N` and `--min-impressions N` (since X API doesn't support these as search operators)
- **Save to file** — `--save` flag auto-saves research to `data/exports/`
- **Multiple output formats** — `--json` for raw data, `--markdown` for research docs, default for Telegram

### 🔧 Changed
- **SKILL.md** rewritten to reference CLI tooling. Research loop instructions preserved and updated.
- **README.md** expanded with full install, setup, usage, and API cost documentation.

### How it compares to v1
- v1 was a prompt-only skill — Claude assembled raw curl commands with inline Python parsers each time
- v2 wraps everything in typed Bun scripts — faster execution, cleaner output, fewer context tokens burned on boilerplate
- Same agentic research loop, same X API, just better tooling underneath

---

## [1.0.0] (2026-02-08)

### ✨ Added
- Initial release
- SKILL.md with agentic research loop (decompose → search → refine → follow threads → deep-dive → synthesize)
- `references/x-api.md` with full X API endpoint reference
- Search operators, pagination, thread following, linked content deep-diving
