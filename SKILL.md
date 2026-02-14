# xint — X Intelligence CLI

Fast, zero-dependency binary for X/Twitter search, analysis, and engagement from the terminal. All output goes to stdout (pipe-friendly).

## Setup

Requires env vars (in `.env` or exported):
- `X_BEARER_TOKEN` — for search, profile, tweet, thread, trends, watch, report
- `X_CLIENT_ID` — for OAuth commands (bookmarks, likes, following, diff)
- `XAI_API_KEY` — for AI analysis (analyze, report --sentiment)

OAuth setup (one-time): `xint auth setup`

## Commands

### Search & Discovery
```bash
xint search "AI agents" --limit 10            # Search recent tweets
xint search "AI agents" --quick               # Fast mode (1 page, 10 max, 1hr cache)
xint search "AI agents" --quality             # Min 10 likes filter
xint search "AI agents" --since 1d --sort likes
xint search "from:elonmusk" --limit 5
xint search "AI agents" --json                # JSON output
xint search "AI agents" --jsonl               # One JSON per line
xint search "AI agents" --csv                 # CSV output
xint search "AI agents" --sentiment           # AI sentiment analysis (needs XAI_API_KEY)
xint search "AI agents" --save                # Save to data/exports/
```

### Monitoring
```bash
xint watch "AI agents" -i 5m                  # Poll every 5 minutes
xint watch "@elonmusk" -i 30s                 # Watch user (auto-expands to from:)
xint watch "bitcoin" --webhook https://...    # POST new tweets to webhook
xint watch "topic" --jsonl                    # Machine-readable output
```

### Profiles & Tweets
```bash
xint profile elonmusk                         # User profile + recent tweets
xint profile elonmusk --json                  # JSON output
xint tweet 1234567890                         # Fetch single tweet
xint thread 1234567890                        # Fetch conversation thread
```

### Trends
```bash
xint trends                                   # Worldwide trending
xint trends us                                # US trends
xint trends --json                            # JSON output
xint trends --locations                       # List supported locations
```

### AI Analysis (requires XAI_API_KEY)
```bash
xint analyze "What's the sentiment around AI?"
xint analyze --tweets saved.json              # Analyze tweets from file
cat tweets.json | xint analyze --pipe         # Analyze from stdin
xint analyze "question" --system "You are..."  # Custom system prompt
```

### Intelligence Reports
```bash
xint report "AI agents"                       # Full report with AI summary
xint report "AI agents" -a @user1,@user2      # Track specific accounts
xint report "AI agents" -s                    # Include sentiment analysis
xint report "AI agents" --save                # Save to data/exports/
```

### Follower Tracking (requires OAuth)
```bash
xint diff @username                           # Snapshot followers, diff vs previous
xint diff @username --following               # Track following instead
xint diff @username --history                 # Show snapshot history
```

### Bookmarks & Engagement (requires OAuth)
```bash
xint bookmarks                                # List bookmarks
xint bookmarks --since 1d                     # Recent bookmarks
xint bookmark 1234567890                      # Save tweet
xint unbookmark 1234567890                    # Remove bookmark
xint likes                                    # List liked tweets
xint like 1234567890                          # Like a tweet
xint unlike 1234567890                        # Unlike a tweet
xint following                                # List accounts you follow
```

### Cost Tracking
```bash
xint costs                                    # Today's API costs
xint costs week                               # Last 7 days
xint costs month                              # Last 30 days
xint costs budget 2.00                        # Set $2/day budget
```

### Watchlist
```bash
xint watchlist                                # List watched accounts
xint watchlist add @username "competitor"     # Add with note
xint watchlist remove @username               # Remove
xint watchlist check @username                # Check if watched
```

### Utilities
```bash
xint auth setup                               # OAuth setup (interactive)
xint auth setup --manual                      # Manual paste mode
xint auth status                              # Show auth info
xint auth refresh                             # Force token refresh
xint cache clear                              # Clear cached data
```

## Output Formats

Most commands support `--json` for raw JSON. Search also supports:
- `--jsonl` — one JSON object per line (great for piping)
- `--csv` — spreadsheet-compatible
- `--markdown` — formatted for reports

## Piping

```bash
xint search "topic" --jsonl | jq '.username'
xint search "topic" --json | xint analyze --pipe "summarize these"
xint search "topic" --csv > export.csv
```

## Cost Awareness

X API costs ~$0.005/tweet read. Budget system prevents runaway costs:
- Default: $1.00/day limit
- Set custom: `xint costs budget <amount>`
- Watch command auto-stops at budget limit
