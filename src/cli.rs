use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "xint", about = "X Intelligence CLI", version)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Search recent tweets
    #[command(alias = "s")]
    Search(SearchArgs),

    /// Monitor X in real-time (polls on interval)
    #[command(alias = "w")]
    Watch(WatchArgs),

    /// Track follower/following changes over time
    #[command(alias = "followers")]
    Diff(DiffArgs),

    /// Generate intelligence report with AI analysis
    Report(ReportArgs),

    /// Fetch full conversation thread
    #[command(alias = "t")]
    Thread(ThreadArgs),

    /// Recent tweets from a user
    #[command(alias = "p")]
    Profile(ProfileArgs),

    /// Fetch a single tweet
    Tweet(TweetArgs),

    /// Fetch and read full article content from a URL
    #[command(alias = "read")]
    Article(ArticleArgs),

    /// Fetch your bookmarked tweets (OAuth required)
    #[command(alias = "bm")]
    Bookmarks(BookmarksArgs),

    /// Bookmark a tweet (OAuth required)
    Bookmark(BookmarkArgs),

    /// Remove a bookmark (OAuth required)
    Unbookmark(UnbookmarkArgs),

    /// Fetch your liked tweets (OAuth required)
    Likes(LikesArgs),

    /// Like a tweet (OAuth required)
    Like(LikeArgs),

    /// Unlike a tweet (OAuth required)
    Unlike(UnlikeArgs),

    /// List accounts you follow (OAuth required)
    Following(FollowingArgs),

    /// Fetch trending topics
    #[command(alias = "tr")]
    Trends(TrendsArgs),

    /// Analyze with Grok AI (xAI)
    #[command(alias = "ask")]
    Analyze(AnalyzeArgs),

    /// View API cost tracking & budget
    #[command(alias = "cost")]
    Costs(CostsArgs),

    /// Manage watchlist
    #[command(alias = "wl")]
    Watchlist(WatchlistArgs),

    /// OAuth 2.0 PKCE authentication
    Auth(AuthArgs),

    /// Cache management
    Cache(CacheArgs),

    /// xAI X Search (Responses API — no cookies/GraphQL)
    #[command(alias = "xs")]
    XSearch(XSearchArgs),

    /// xAI Collections knowledge base management
    #[command(alias = "col")]
    Collections(CollectionsArgs),

    /// Start MCP server for AI agents (Claude, OpenAI)
    #[command(alias = "mcp-server")]
    Mcp(McpArgs),
}

// ---------------------------------------------------------------------------
// Search
// ---------------------------------------------------------------------------

#[derive(Parser)]
pub struct SearchArgs {
    /// Search query
    pub query: Vec<String>,

    /// Sort order: likes, impressions, retweets, recent
    #[arg(long, default_value = "likes")]
    pub sort: String,

    /// Filter by minimum likes
    #[arg(long, default_value = "0")]
    pub min_likes: u64,

    /// Filter by minimum impressions
    #[arg(long, default_value = "0")]
    pub min_impressions: u64,

    /// Pages to fetch (1-5)
    #[arg(long, default_value = "1")]
    pub pages: u32,

    /// Max results to display
    #[arg(long, default_value = "15")]
    pub limit: usize,

    /// Time filter (1h, 3h, 12h, 1d, 7d)
    #[arg(long)]
    pub since: Option<String>,

    /// End time filter (full-archive only)
    #[arg(long)]
    pub until: Option<String>,

    /// Full-archive search
    #[arg(long)]
    pub full: bool,

    /// Exclude replies
    #[arg(long)]
    pub no_replies: bool,

    /// Exclude retweets
    #[arg(long)]
    pub no_retweets: bool,

    /// Quick mode: 1 page, max 10, noise filter, 1hr cache
    #[arg(long)]
    pub quick: bool,

    /// Pre-filter low-engagement tweets (min 10 likes)
    #[arg(long)]
    pub quality: bool,

    /// Shorthand for from:username
    #[arg(long)]
    pub from: Option<String>,

    /// AI sentiment analysis via Grok
    #[arg(long)]
    pub sentiment: bool,

    /// Save results to data/exports/
    #[arg(long)]
    pub save: bool,

    /// Raw JSON output
    #[arg(long)]
    pub json: bool,

    /// JSONL output (one tweet per line)
    #[arg(long)]
    pub jsonl: bool,

    /// CSV output
    #[arg(long)]
    pub csv: bool,

    /// Markdown output
    #[arg(long)]
    pub markdown: bool,
}

// ---------------------------------------------------------------------------
// Watch
// ---------------------------------------------------------------------------

#[derive(Parser)]
pub struct WatchArgs {
    /// Search query
    pub query: Vec<String>,

    /// Polling interval: 30s, 5m, 1h
    #[arg(long, short = 'i', default_value = "5m")]
    pub interval: String,

    /// POST new tweets to this URL
    #[arg(long)]
    pub webhook: Option<String>,

    /// Max tweets per poll
    #[arg(long, default_value = "10")]
    pub limit: usize,

    /// Initial time window
    #[arg(long, default_value = "1h")]
    pub since: String,

    /// Suppress per-poll headers
    #[arg(long, short = 'q')]
    pub quiet: bool,

    /// Output JSONL
    #[arg(long)]
    pub jsonl: bool,
}

// ---------------------------------------------------------------------------
// Diff
// ---------------------------------------------------------------------------

#[derive(Parser)]
pub struct DiffArgs {
    /// Username to track
    pub username: Option<String>,

    /// Track following instead of followers
    #[arg(long)]
    pub following: bool,

    /// Show snapshot history
    #[arg(long)]
    pub history: bool,

    /// Output as JSON
    #[arg(long)]
    pub json: bool,

    /// Max pages to fetch (default: 5)
    #[arg(long, default_value = "5")]
    pub pages: u32,
}

// ---------------------------------------------------------------------------
// Report
// ---------------------------------------------------------------------------

#[derive(Parser)]
pub struct ReportArgs {
    /// Report topic
    pub topic: Vec<String>,

    /// Comma-separated accounts to track
    #[arg(long, short = 'a')]
    pub accounts: Option<String>,

    /// Include sentiment analysis
    #[arg(long, short = 's')]
    pub sentiment: bool,

    /// Grok model
    #[arg(long, default_value = "grok-3-mini")]
    pub model: String,

    /// Search pages
    #[arg(long, default_value = "2")]
    pub pages: u32,

    /// Save report
    #[arg(long)]
    pub save: bool,
}

// ---------------------------------------------------------------------------
// Simple commands
// ---------------------------------------------------------------------------

#[derive(Parser)]
pub struct ThreadArgs {
    /// Tweet ID
    pub tweet_id: String,

    /// Pages to fetch
    #[arg(long, default_value = "2")]
    pub pages: u32,
}

#[derive(Parser)]
pub struct ProfileArgs {
    /// Username
    pub username: String,

    /// Number of tweets
    #[arg(long, default_value = "20")]
    pub count: u32,

    /// Include replies
    #[arg(long)]
    pub replies: bool,

    /// JSON output
    #[arg(long)]
    pub json: bool,
}

#[derive(Parser)]
pub struct TweetArgs {
    /// Tweet ID
    pub tweet_id: String,

    /// JSON output
    #[arg(long)]
    pub json: bool,
}

#[derive(Parser)]
pub struct ArticleArgs {
    /// URL to fetch article from (or X tweet URL for auto-extraction)
    pub url: String,

    /// JSON output
    #[arg(long)]
    pub json: bool,

    /// Full article text (no truncation)
    #[arg(long)]
    pub full: bool,

    /// Grok model (default: grok-4 for article fetching)
    #[arg(long, default_value = "grok-4")]
    pub model: String,

    /// Analyze article with Grok AI - ask a question about the content
    #[arg(long, short = 'a')]
    pub ai: Option<String>,
}

// ---------------------------------------------------------------------------
// Bookmarks
// ---------------------------------------------------------------------------

#[derive(Parser)]
pub struct BookmarksArgs {
    /// Max bookmarks to display
    #[arg(long, default_value = "20")]
    pub limit: usize,

    /// Filter by recency
    #[arg(long)]
    pub since: Option<String>,

    /// Client-side text filter
    #[arg(long)]
    pub query: Option<String>,

    /// JSON output
    #[arg(long)]
    pub json: bool,

    /// Markdown output
    #[arg(long)]
    pub markdown: bool,

    /// Save to exports
    #[arg(long)]
    pub save: bool,

    /// Skip cache
    #[arg(long)]
    pub no_cache: bool,
}

#[derive(Parser)]
pub struct BookmarkArgs {
    /// Tweet ID to bookmark
    pub tweet_id: String,
}

#[derive(Parser)]
pub struct UnbookmarkArgs {
    /// Tweet ID to unbookmark
    pub tweet_id: String,
}

// ---------------------------------------------------------------------------
// Likes
// ---------------------------------------------------------------------------

#[derive(Parser)]
pub struct LikesArgs {
    /// Max likes to display
    #[arg(long, default_value = "20")]
    pub limit: usize,

    /// Filter by recency
    #[arg(long)]
    pub since: Option<String>,

    /// Client-side text filter
    #[arg(long)]
    pub query: Option<String>,

    /// JSON output
    #[arg(long)]
    pub json: bool,

    /// Markdown output
    #[arg(long)]
    pub markdown: bool,

    /// Skip cache
    #[arg(long)]
    pub no_cache: bool,
}

#[derive(Parser)]
pub struct LikeArgs {
    /// Tweet ID to like
    pub tweet_id: String,
}

#[derive(Parser)]
pub struct UnlikeArgs {
    /// Tweet ID to unlike
    pub tweet_id: String,
}

// ---------------------------------------------------------------------------
// Following
// ---------------------------------------------------------------------------

#[derive(Parser)]
pub struct FollowingArgs {
    /// Username (defaults to authenticated user)
    pub username: Option<String>,

    /// Max accounts to display
    #[arg(long, default_value = "50")]
    pub limit: usize,

    /// JSON output
    #[arg(long)]
    pub json: bool,
}

// ---------------------------------------------------------------------------
// Trends
// ---------------------------------------------------------------------------

#[derive(Parser)]
pub struct TrendsArgs {
    /// Location name or WOEID
    pub location: Option<Vec<String>>,

    /// Number of trends to display
    #[arg(long, short = 'n', default_value = "20")]
    pub limit: usize,

    /// JSON output
    #[arg(long)]
    pub json: bool,

    /// Bypass cache
    #[arg(long)]
    pub no_cache: bool,

    /// List known locations
    #[arg(long)]
    pub locations: bool,
}

// ---------------------------------------------------------------------------
// Analyze
// ---------------------------------------------------------------------------

#[derive(Parser)]
pub struct AnalyzeArgs {
    /// Query or question
    pub query: Vec<String>,

    /// Grok model
    #[arg(long, default_value = "grok-3-mini")]
    pub model: String,

    /// Custom system prompt
    #[arg(long)]
    pub system: Option<String>,

    /// Analyze tweets from JSON file
    #[arg(long)]
    pub tweets: Option<String>,

    /// Read tweet JSON from stdin
    #[arg(long)]
    pub pipe: bool,
}

// ---------------------------------------------------------------------------
// Costs
// ---------------------------------------------------------------------------

#[derive(Parser)]
pub struct CostsArgs {
    /// Subcommand: today, week, month, all, budget, reset
    pub subcommand: Option<Vec<String>>,
}

// ---------------------------------------------------------------------------
// Watchlist
// ---------------------------------------------------------------------------

#[derive(Parser)]
pub struct WatchlistArgs {
    /// Subcommand: add, remove, check, or empty for list
    pub subcommand: Option<Vec<String>>,
}

// ---------------------------------------------------------------------------
// Auth
// ---------------------------------------------------------------------------

#[derive(Parser)]
pub struct AuthArgs {
    /// Subcommand: setup, status, refresh
    pub subcommand: Option<String>,

    /// Manual mode for auth setup
    #[arg(long)]
    pub manual: bool,
}

// ---------------------------------------------------------------------------
// Cache
// ---------------------------------------------------------------------------

#[derive(Parser)]
pub struct CacheArgs {
    /// Subcommand: clear
    pub subcommand: Option<String>,
}

// ---------------------------------------------------------------------------
// X Search (xAI Responses API)
// ---------------------------------------------------------------------------

#[derive(Parser)]
pub struct XSearchArgs {
    /// Workspace root (for memory candidate dedup)
    #[arg(long, default_value = ".")]
    pub workspace: String,

    /// JSON file with queries: {"queries": [...]} or [...]
    #[arg(long)]
    pub queries_file: String,

    /// Path to write markdown report
    #[arg(long, default_value = "x-search-report.md")]
    pub out_md: String,

    /// Path to write raw JSON payload
    #[arg(long, default_value = "x-search-raw.json")]
    pub out_json: String,

    /// Append new memory candidates (deduped by source)
    #[arg(long)]
    pub emit_candidates: bool,

    /// Candidate JSONL output path
    #[arg(long, default_value = "")]
    pub candidates_out: String,

    /// xAI model
    #[arg(long, default_value = "grok-4")]
    pub model: String,

    /// Max search results per query
    #[arg(long, default_value = "10")]
    pub max_results: u32,

    /// Timeout in seconds per query
    #[arg(long, default_value = "45")]
    pub timeout_seconds: u32,

    /// From date filter (YYYY-MM-DD)
    #[arg(long)]
    pub from_date: Option<String>,

    /// To date filter (YYYY-MM-DD)
    #[arg(long)]
    pub to_date: Option<String>,
}

// ---------------------------------------------------------------------------
// Collections (xAI Management API)
// ---------------------------------------------------------------------------

#[derive(Parser)]
pub struct CollectionsArgs {
    /// Subcommand: list, create, ensure, add-document, upload, search, sync-dir
    pub subcommand: Option<Vec<String>>,

    /// Top-K results for document search
    #[arg(long, default_value = "8")]
    pub top_k: u32,
}

// ---------------------------------------------------------------------------
// MCP Server
// ---------------------------------------------------------------------------

#[derive(Parser)]
pub struct McpArgs {
    /// Run in SSE mode (HTTP server)
    #[arg(long)]
    pub sse: bool,
    
    /// Port for SSE mode (default: 3000)
    #[arg(long, default_value = "3000")]
    pub port: u16,
}
