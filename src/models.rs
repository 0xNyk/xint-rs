#![allow(dead_code)]

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ---------------------------------------------------------------------------
// Tweet
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TweetMetrics {
    pub likes: u64,
    pub retweets: u64,
    pub replies: u64,
    pub quotes: u64,
    pub impressions: u64,
    pub bookmarks: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UrlEntity {
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unwound_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub images: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tweet {
    pub id: String,
    pub text: String,
    pub author_id: String,
    pub username: String,
    pub name: String,
    pub created_at: String,
    pub conversation_id: String,
    pub metrics: TweetMetrics,
    pub urls: Vec<UrlEntity>,
    pub mentions: Vec<String>,
    pub hashtags: Vec<String>,
    pub tweet_url: String,
}

// ---------------------------------------------------------------------------
// Raw API response
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct RawUser {
    pub id: String,
    pub username: Option<String>,
    pub name: Option<String>,
    pub public_metrics: Option<UserPublicMetrics>,
    pub description: Option<String>,
    pub created_at: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UserPublicMetrics {
    pub followers_count: Option<u64>,
    pub following_count: Option<u64>,
    pub tweet_count: Option<u64>,
    pub listed_count: Option<u64>,
}

#[derive(Debug, Deserialize)]
pub struct RawTweetMetrics {
    pub like_count: Option<u64>,
    pub retweet_count: Option<u64>,
    pub reply_count: Option<u64>,
    pub quote_count: Option<u64>,
    pub impression_count: Option<u64>,
    pub bookmark_count: Option<u64>,
}

#[derive(Debug, Deserialize)]
pub struct RawUrl {
    pub expanded_url: Option<String>,
    pub unwound_url: Option<String>,
    pub title: Option<String>,
    pub description: Option<String>,
    pub images: Option<Vec<RawUrlImage>>,
}

#[derive(Debug, Deserialize)]
pub struct RawUrlImage {
    pub url: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct RawMention {
    pub username: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct RawHashtag {
    pub tag: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct RawEntities {
    pub urls: Option<Vec<RawUrl>>,
    pub mentions: Option<Vec<RawMention>>,
    pub hashtags: Option<Vec<RawHashtag>>,
}

#[derive(Debug, Deserialize)]
pub struct RawTweet {
    pub id: String,
    pub text: String,
    pub author_id: Option<String>,
    pub created_at: Option<String>,
    pub conversation_id: Option<String>,
    pub public_metrics: Option<RawTweetMetrics>,
    pub entities: Option<RawEntities>,
}

#[derive(Debug, Deserialize)]
pub struct RawIncludes {
    pub users: Option<Vec<RawUser>>,
    pub media: Option<Vec<serde_json::Value>>,
}

#[derive(Debug, Deserialize)]
pub struct RawMeta {
    pub next_token: Option<String>,
    pub result_count: Option<u64>,
}

#[derive(Debug, Deserialize)]
pub struct RawResponse {
    pub data: Option<serde_json::Value>,
    pub includes: Option<RawIncludes>,
    pub meta: Option<RawMeta>,
    pub errors: Option<Vec<serde_json::Value>>,
    pub title: Option<String>,
    pub detail: Option<String>,
    pub status: Option<u16>,
}

// ---------------------------------------------------------------------------
// OAuth
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuthTokens {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_at: i64, // unix ms
    pub user_id: String,
    pub username: String,
    pub scope: String,
    pub created_at: String,
    pub refreshed_at: String,
}

// ---------------------------------------------------------------------------
// Costs
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CostEntry {
    pub timestamp: String,
    pub operation: String,
    pub endpoint: String,
    pub tweets_read: u64,
    pub cost_usd: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperationStats {
    pub calls: u64,
    pub cost: f64,
    pub tweets: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DailyAggregate {
    pub date: String,
    pub total_cost: f64,
    pub calls: u64,
    pub tweets_read: u64,
    pub by_operation: HashMap<String, OperationStats>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BudgetConfig {
    pub daily_limit_usd: f64,
    pub warn_threshold: f64,
    pub enabled: bool,
}

impl Default for BudgetConfig {
    fn default() -> Self {
        Self {
            daily_limit_usd: 1.0,
            warn_threshold: 0.8,
            enabled: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CostData {
    pub entries: Vec<CostEntry>,
    pub daily: Vec<DailyAggregate>,
    pub budget: BudgetConfig,
    pub total_lifetime_usd: f64,
}

impl Default for CostData {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
            daily: Vec::new(),
            budget: BudgetConfig::default(),
            total_lifetime_usd: 0.0,
        }
    }
}

pub struct BudgetStatus {
    pub allowed: bool,
    pub spent: f64,
    pub limit: f64,
    pub remaining: f64,
    pub warning: bool,
}

// ---------------------------------------------------------------------------
// Trends
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Trend {
    pub name: String,
    pub tweet_count: Option<u64>,
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrendsResult {
    pub source: String, // "api" or "search_fallback"
    pub location: String,
    pub woeid: u32,
    pub trends: Vec<Trend>,
    pub fetched_at: String,
}

// ---------------------------------------------------------------------------
// Grok
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct GrokMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone)]
pub struct GrokOpts {
    pub model: String,
    pub temperature: f64,
    pub max_tokens: u32,
}

impl Default for GrokOpts {
    fn default() -> Self {
        Self {
            model: "grok-3-mini".to_string(),
            temperature: 0.7,
            max_tokens: 1024,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GrokResponse {
    pub content: String,
    pub model: String,
    pub usage: GrokUsage,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GrokUsage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
}

// ---------------------------------------------------------------------------
// Article
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Article {
    pub url: String,
    pub title: String,
    pub description: String,
    pub content: String,
    pub author: String,
    pub published: String,
    pub domain: String,
    pub ttr: u64, // time to read in minutes
    pub word_count: u64,
}

// ---------------------------------------------------------------------------
// Sentiment
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SentimentResult {
    pub id: String,
    pub sentiment: String, // positive, negative, neutral, mixed
    pub score: f64,        // -1.0 to 1.0
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

#[derive(Debug, Clone)]
pub struct SentimentStats {
    pub positive: u32,
    pub negative: u32,
    pub neutral: u32,
    pub mixed: u32,
    pub average_score: f64,
}

// ---------------------------------------------------------------------------
// Followers / Diff
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserSnapshot {
    pub id: String,
    pub username: String,
    pub name: String,
    pub followers_count: Option<u64>,
    pub following_count: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snapshot {
    pub username: String,
    #[serde(rename = "type")]
    pub snap_type: String, // "followers" or "following"
    pub timestamp: String,
    pub count: usize,
    pub users: Vec<UserSnapshot>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FollowerDiff {
    pub added: Vec<UserSnapshot>,
    pub removed: Vec<UserSnapshot>,
    pub unchanged: usize,
    pub previous: DiffSnapshot,
    pub current: DiffSnapshot,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffSnapshot {
    pub timestamp: String,
    pub count: usize,
}

// ---------------------------------------------------------------------------
// Watchlist
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatchlistAccount {
    pub username: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    #[serde(rename = "addedAt")]
    pub added_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Watchlist {
    pub accounts: Vec<WatchlistAccount>,
}
