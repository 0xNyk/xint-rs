# xint-rs Rust Codebase Guide

## Project Overview

- **Lines**: ~10,290 LOC across 20+ modules
- **Runtime**: Rust with Tokio async
- **Entry**: `src/main.rs`

## File Structure

```
src/
├── main.rs           # CLI entry point
├── cli.rs            # Command definitions (clap)
├── client.rs         # HTTP client (reqwest)
├── config.rs         # Configuration loading
├── cache.rs          # Caching
├── costs.rs          # Cost tracking
├── format.rs         # Output formatting
├── models.rs         # Data models
├── sentiment.rs      # Sentiment analysis
├── mcp.rs            # MCP server
│
├── api/
│   ├── mod.rs        # API module exports
│   ├── grok.rs       # xAI Grok client
│   ├── twitter.rs    # X API client
│   └── xai.rs       # xAI utilities
│
├── auth/
│   ├── mod.rs        # Auth exports
│   └── oauth.rs      # OAuth 2.0
│
└── commands/
    ├── mod.rs        # Command exports
    ├── search.rs     # Search command
    ├── profile.rs    # Profile command
    ├── thread.rs     # Thread command
    ├── tweet.rs      # Tweet command
    ├── article.rs    # Article command
    ├── bookmarks.rs  # Bookmarks command
    ├── engagement.rs # Likes, following
    ├── trends.rs     # Trending topics
    ├── analyze.rs    # Grok analysis
    ├── report.rs    # Intelligence reports
    ├── watch.rs     # Real-time monitoring
    ├── diff.rs      # Follower tracking
    ├── watchlist.rs # Watchlist management
    ├── auth_cmd.rs  # OAuth commands
    ├── cache_cmd.rs # Cache commands
    ├── costs_cmd.rs # Cost commands
    ├── x_search.rs  # xAI x-search
    └── collections.rs # Knowledge base
```

## Key Patterns

### Adding Commands

1. Create `src/commands/newcmd.rs`
2. Add to `src/commands/mod.rs`:
   ```rust
   pub mod newcmd;
   ```
3. Add variant to `Commands` enum in `src/cli.rs`
4. Add handler in `src/main.rs`

### Error Handling
- Use `anyhow::Result` for main functions
- `thiserror` for specific error types

### Async
- All network calls use `async/await`
- Tokio runtime in `main.rs`

## Dependencies

```toml
tokio = { version = "1", features = ["full"] }
reqwest = { version = "0.12", features = ["json", "rustls-tls"] }
clap = { version = "4", features = ["derive"] }
serde = { version = "1", features = ["derive"] }
```

## Building

```bash
# Development
cargo build

# Release (optimized)
cargo build --release

# Single binary
cargo build --release -p xint
```
