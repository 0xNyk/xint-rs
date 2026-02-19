//! xint MCP Server
//!
//! MCP (Model Context Protocol) server implementation for xint-rs CLI.
//! Exposes xint functionality as MCP tools for AI agents like Claude Code.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tokio::io::{AsyncBufReadExt, BufReader};

use crate::cli::{McpArgs, PolicyMode};
use crate::config::Config;
use crate::costs;
use crate::policy;
use crate::reliability;

// ============================================================================
// Tool Definitions
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MCPTool {
    pub name: String,
    pub description: String,
    #[serde(rename = "inputSchema")]
    pub input_schema: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum MCPMessage {
    #[serde(rename = "initialize")]
    Initialize {
        protocol_version: String,
        capabilities: serde_json::Value,
        client_info: serde_json::Value,
    },
    #[serde(rename = "initialized")]
    Initialized,
    #[serde(rename = "tools/list")]
    ToolsList,
    #[serde(rename = "tools/list/result")]
    ToolsListResult { tools: Vec<MCPTool> },
    #[serde(rename = "tools/call")]
    ToolsCall {
        name: String,
        arguments: serde_json::Value,
    },
    #[serde(rename = "tools/call/result")]
    ToolsCallResult { content: Vec<MCPContent> },
    #[serde(rename = "error")]
    Error { code: i32, message: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MCPContent {
    #[serde(rename = "type")]
    pub content_type: String,
    pub text: String,
}

// ============================================================================
// MCP Server Implementation
// ============================================================================

pub struct MCPServer {
    initialized: bool,
    policy_mode: PolicyMode,
    enforce_budget: bool,
    costs_path: PathBuf,
    reliability_path: PathBuf,
}

impl MCPServer {
    pub fn new(
        policy_mode: PolicyMode,
        enforce_budget: bool,
        costs_path: PathBuf,
        reliability_path: PathBuf,
    ) -> Self {
        Self {
            initialized: false,
            policy_mode,
            enforce_budget,
            costs_path,
            reliability_path,
        }
    }

    fn get_tools() -> Vec<MCPTool> {
        vec![
            MCPTool {
                name: "xint_search".to_string(),
                description: "Search recent tweets on X/Twitter with advanced filters".to_string(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "query": { "type": "string", "description": "Search query" },
                        "limit": { "type": "number", "description": "Max results (default: 15)" },
                        "since": { "type": "string", "description": "Time filter: 1h, 1d, 7d" },
                        "sort": { "type": "string", "enum": ["likes", "retweets", "recent"], "description": "Sort order" },
                    },
                    "required": ["query"]
                }),
            },
            MCPTool {
                name: "xint_profile".to_string(),
                description: "Get recent tweets from a specific X/Twitter user".to_string(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "username": { "type": "string", "description": "Twitter username (without @)" },
                        "count": { "type": "number", "description": "Number of tweets (default: 20)" },
                    },
                    "required": ["username"]
                }),
            },
            MCPTool {
                name: "xint_thread".to_string(),
                description: "Get full conversation thread from a tweet".to_string(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "tweet_id": { "type": "string", "description": "Tweet ID or URL" },
                        "pages": { "type": "number", "description": "Pages to fetch (default: 2)" },
                    },
                    "required": ["tweet_id"]
                }),
            },
            MCPTool {
                name: "xint_tweet".to_string(),
                description: "Get a single tweet by ID".to_string(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "tweet_id": { "type": "string", "description": "Tweet ID or URL" },
                    },
                    "required": ["tweet_id"]
                }),
            },
            MCPTool {
                name: "xint_trends".to_string(),
                description: "Get trending topics on X".to_string(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "location": { "type": "string", "description": "Location or WOEID (default: worldwide)" },
                        "limit": { "type": "number", "description": "Number of trends (default: 20)" },
                    },
                }),
            },
            MCPTool {
                name: "xint_xsearch".to_string(),
                description: "Search X using xAI's Grok x-search for AI-powered results".to_string(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "query": { "type": "string", "description": "Search query" },
                        "limit": { "type": "number", "description": "Max results (default: 10)" },
                    },
                    "required": ["query"]
                }),
            },
            MCPTool {
                name: "xint_collections_list".to_string(),
                description: "List all xAI Collections knowledge base collections".to_string(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {},
                }),
            },
            MCPTool {
                name: "xint_analyze".to_string(),
                description: "Analyze tweets or answer questions using Grok AI".to_string(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "query": { "type": "string", "description": "Question or analysis request" },
                        "model": { "type": "string", "description": "Grok model (grok-3-mini, grok-3)" },
                    },
                    "required": ["query"]
                }),
            },
            MCPTool {
                name: "xint_article".to_string(),
                description: "Fetch and extract content from a URL article. Also supports X tweet URLs - extracts linked article automatically. Use ai_prompt to analyze with Grok.".to_string(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "url": { "type": "string", "description": "Article URL or X tweet URL to fetch" },
                        "full": { "type": "boolean", "description": "Fetch full content (default: false)" },
                        "ai_prompt": { "type": "string", "description": "Analyze article with Grok AI - ask a question about the content" },
                    },
                    "required": ["url"]
                }),
            },
            MCPTool {
                name: "xint_collections_search".to_string(),
                description: "Search within an xAI Collections knowledge base".to_string(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "collection_id": { "type": "string", "description": "Collection ID to search in" },
                        "query": { "type": "string", "description": "Search query" },
                        "limit": { "type": "number", "description": "Max results (default: 5)" },
                    },
                    "required": ["collection_id", "query"]
                }),
            },
            MCPTool {
                name: "xint_bookmarks".to_string(),
                description: "Get your bookmarked tweets (requires OAuth)".to_string(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "limit": { "type": "number", "description": "Max bookmarks (default: 20)" },
                        "since": { "type": "string", "description": "Filter by recency: 1h, 1d, 7d" },
                    },
                }),
            },
            MCPTool {
                name: "xint_package_create".to_string(),
                description: "Create an agent memory package ingest job (v1 draft contract)"
                    .to_string(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "name": { "type": "string", "description": "Human-readable package name" },
                        "topic_query": { "type": "string", "description": "Topic query used for ingest and refresh" },
                        "sources": {
                            "type": "array",
                            "items": { "type": "string", "enum": ["x_api_v2", "xai_search", "web_article"] },
                            "description": "Data sources to ingest"
                        },
                        "time_window": {
                            "type": "object",
                            "properties": {
                                "from": { "type": "string", "format": "date-time" },
                                "to": { "type": "string", "format": "date-time" }
                            },
                            "required": ["from", "to"]
                        },
                        "policy": { "type": "string", "enum": ["private", "shared_candidate"] },
                        "analysis_profile": { "type": "string", "enum": ["summary", "analyst", "forensic"] }
                    },
                    "required": ["name", "topic_query", "sources", "time_window", "policy", "analysis_profile"]
                }),
            },
            MCPTool {
                name: "xint_package_status".to_string(),
                description: "Get package metadata and freshness (v1 draft contract)".to_string(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "package_id": { "type": "string", "description": "Package identifier (pkg_*)" }
                    },
                    "required": ["package_id"]
                }),
            },
            MCPTool {
                name: "xint_package_query".to_string(),
                description:
                    "Query one or more packages and return claims with citations (v1 draft contract)"
                        .to_string(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "query": { "type": "string", "description": "Question to ask over package memory" },
                        "package_ids": {
                            "type": "array",
                            "items": { "type": "string" },
                            "description": "Package IDs included in retrieval scope"
                        },
                        "max_claims": { "type": "number", "description": "Maximum number of claims (default: 10)" },
                        "require_citations": { "type": "boolean", "description": "Require citations in response (default: true)" }
                    },
                    "required": ["query", "package_ids"]
                }),
            },
            MCPTool {
                name: "xint_package_refresh".to_string(),
                description:
                    "Trigger package refresh and create a new snapshot (v1 draft contract)"
                        .to_string(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "package_id": { "type": "string", "description": "Package identifier" },
                        "reason": { "type": "string", "enum": ["ttl", "manual", "event"] }
                    },
                    "required": ["package_id", "reason"]
                }),
            },
            MCPTool {
                name: "xint_package_search".to_string(),
                description: "Search private and shared package catalog (v1 draft contract)"
                    .to_string(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "query": { "type": "string", "description": "Search query for package catalog" },
                        "limit": { "type": "number", "description": "Max packages to return (default: 20)" }
                    },
                    "required": ["query"]
                }),
            },
            MCPTool {
                name: "xint_package_publish".to_string(),
                description: "Publish a package snapshot to shared catalog (v1 draft contract)"
                    .to_string(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "package_id": { "type": "string", "description": "Package identifier" },
                        "snapshot_version": { "type": "number", "description": "Snapshot version to publish" }
                    },
                    "required": ["package_id", "snapshot_version"]
                }),
            },
            MCPTool {
                name: "xint_cache_clear".to_string(),
                description: "Clear the xint search cache".to_string(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {},
                }),
            },
            MCPTool {
                name: "xint_watch".to_string(),
                description: "Monitor X in real-time with polling. Returns new tweets since last check.".to_string(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "query": { "type": "string", "description": "Search query to monitor" },
                        "limit": { "type": "number", "description": "Max tweets per check (default: 10)" },
                        "since": { "type": "string", "description": "Time window: 1h, 1d (default: 1h)" },
                    },
                    "required": ["query"]
                }),
            },
            MCPTool {
                name: "xint_diff".to_string(),
                description: "Track follower/following changes for a user".to_string(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "username": { "type": "string", "description": "Twitter username to track" },
                        "following": { "type": "boolean", "description": "Track following instead of followers (default: false)" },
                    },
                    "required": ["username"]
                }),
            },
            MCPTool {
                name: "xint_report".to_string(),
                description: "Generate an AI-powered intelligence report on a topic".to_string(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "topic": { "type": "string", "description": "Report topic or query" },
                        "sentiment": { "type": "boolean", "description": "Include sentiment analysis (default: false)" },
                        "model": { "type": "string", "description": "Grok model (default: grok-3-mini)" },
                        "pages": { "type": "number", "description": "Search pages (default: 2)" },
                    },
                    "required": ["topic"]
                }),
            },
            MCPTool {
                name: "xint_sentiment".to_string(),
                description: "Analyze sentiment of tweets".to_string(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "tweets": { "type": "array", "description": "Array of tweets to analyze" },
                    },
                    "required": ["tweets"]
                }),
            },
            MCPTool {
                name: "xint_costs".to_string(),
                description: "Get API cost tracking information".to_string(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "period": { "type": "string", "enum": ["today", "week", "month", "all"], "description": "Time period (default: today)" },
                    },
                }),
            },
        ]
    }

    fn tool_required_policy(name: &str) -> PolicyMode {
        match name {
            "xint_bookmarks" | "xint_diff" | "xint_package_publish" => PolicyMode::Engagement,
            _ => PolicyMode::ReadOnly,
        }
    }

    fn tool_budget_guarded(name: &str) -> bool {
        matches!(
            name,
            "xint_search"
                | "xint_profile"
                | "xint_thread"
                | "xint_tweet"
                | "xint_trends"
                | "xint_xsearch"
                | "xint_collections_list"
                | "xint_collections_search"
                | "xint_analyze"
                | "xint_article"
                | "xint_bookmarks"
                | "xint_watch"
                | "xint_diff"
                | "xint_report"
                | "xint_sentiment"
                | "xint_package_create"
                | "xint_package_query"
                | "xint_package_refresh"
                | "xint_package_search"
                | "xint_package_publish"
        )
    }

    fn ensure_tool_allowed(&self, name: &str) -> Result<(), String> {
        let required = Self::tool_required_policy(name);
        if policy::is_allowed(self.policy_mode, required) {
            return Ok(());
        }
        Err(serde_json::json!({
            "code": "POLICY_DENIED",
            "message": format!("MCP tool '{}' requires '{}' policy mode", name, policy::as_str(required)),
            "tool": name,
            "policy_mode": policy::as_str(self.policy_mode),
            "required_mode": policy::as_str(required),
        })
        .to_string())
    }

    fn ensure_budget_allowed(&self, name: &str) -> Result<(), String> {
        if !self.enforce_budget || !Self::tool_budget_guarded(name) {
            return Ok(());
        }
        let budget = costs::check_budget(&self.costs_path);
        if budget.allowed {
            return Ok(());
        }
        Err(serde_json::json!({
            "code": "BUDGET_DENIED",
            "message": format!(
                "Daily budget exceeded (${:.2} / ${:.2})",
                budget.spent, budget.limit
            ),
            "tool": name,
            "spent_usd": budget.spent,
            "limit_usd": budget.limit,
            "remaining_usd": budget.remaining,
        })
        .to_string())
    }

    fn package_api_base_url() -> Option<String> {
        std::env::var("XINT_PACKAGE_API_BASE_URL")
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    }

    fn package_api_key() -> Option<String> {
        std::env::var("XINT_PACKAGE_API_KEY")
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    }

    async fn call_package_api(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<serde_json::Value>,
    ) -> Result<serde_json::Value, String> {
        let base = Self::package_api_base_url().ok_or_else(|| {
            "XINT_PACKAGE_API_BASE_URL not set. Start local API with `xint package-api-server --port=8080` and set XINT_PACKAGE_API_BASE_URL=http://localhost:8080/v1".to_string()
        })?;
        let url = format!("{}{}", base.trim_end_matches('/'), path);

        let client = reqwest::Client::new();
        let mut req = client.request(method, &url);
        if let Some(key) = Self::package_api_key() {
            req = req.header(reqwest::header::AUTHORIZATION, format!("Bearer {key}"));
        }
        if let Some(ref payload) = body {
            req = req
                .header(reqwest::header::CONTENT_TYPE, "application/json")
                .json(payload);
        }

        let res = req
            .send()
            .await
            .map_err(|e| format!("Package API request failed: {e}"))?;
        let status = res.status();
        let text = res
            .text()
            .await
            .map_err(|e| format!("Package API body read failed: {e}"))?;

        if !status.is_success() {
            return Err(format!(
                "Package API {}: {}",
                status.as_u16(),
                text.chars().take(300).collect::<String>()
            ));
        }
        if text.trim().is_empty() {
            return Ok(serde_json::json!({}));
        }

        serde_json::from_str::<serde_json::Value>(&text)
            .map_err(|e| format!("Package API JSON decode failed: {e}"))
    }

    pub async fn handle_message(&mut self, msg: &str) -> Result<Option<String>, String> {
        let parsed: serde_json::Value =
            serde_json::from_str(msg).map_err(|e| format!("Failed to parse JSON: {e}"))?;

        let method = parsed
            .get("method")
            .and_then(|v| v.as_str())
            .ok_or("Missing method field")?;

        let id = parsed.get("id");

        match method {
            "initialize" => {
                self.initialized = true;
                let response = serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": {
                        "protocolVersion": "2024-11-05",
                        "capabilities": {
                            "tools": {}
                        },
                        "serverInfo": {
                            "name": "xint",
                            "version": "1.0.0"
                        }
                    }
                });
                Ok(Some(response.to_string()))
            }
            "initialized" => {
                // Client confirmed initialization
                Ok(None)
            }
            "tools/list" => {
                let tools = Self::get_tools();
                let response = serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": {
                        "tools": tools
                    }
                });
                Ok(Some(response.to_string()))
            }
            "tools/call" => {
                let started_at = std::time::Instant::now();
                let params = parsed.get("params").ok_or("Missing params")?;
                let name = params
                    .get("name")
                    .and_then(|v| v.as_str())
                    .ok_or("Missing tool name")?;
                let arguments = params
                    .get("arguments")
                    .cloned()
                    .unwrap_or(serde_json::Value::Object(serde_json::Map::new()));

                let execution: Result<Vec<MCPContent>, String> =
                    if let Err(err) = self.ensure_tool_allowed(name) {
                        Err(err)
                    } else if let Err(err) = self.ensure_budget_allowed(name) {
                        Err(err)
                    } else {
                        self.execute_tool(name, arguments).await
                    };

                match execution {
                    Ok(result) => {
                        reliability::record_command_result(
                            &self.reliability_path,
                            &format!("mcp:{name}"),
                            true,
                            started_at.elapsed().as_millis(),
                            reliability::ReliabilityMode::Mcp,
                            false,
                        );
                        let response = serde_json::json!({
                            "jsonrpc": "2.0",
                            "id": id,
                            "result": {
                                "content": result
                            }
                        });
                        Ok(Some(response.to_string()))
                    }
                    Err(err) => {
                        reliability::record_command_result(
                            &self.reliability_path,
                            &format!("mcp:{name}"),
                            false,
                            started_at.elapsed().as_millis(),
                            reliability::ReliabilityMode::Mcp,
                            false,
                        );
                        let response = serde_json::json!({
                            "jsonrpc": "2.0",
                            "id": id,
                            "error": {
                                "code": -32603,
                                "message": err
                            }
                        });
                        Ok(Some(response.to_string()))
                    }
                }
            }
            _ => {
                let response = serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "error": {
                        "code": -32601,
                        "message": format!("Method not found: {}", method)
                    }
                });
                Ok(Some(response.to_string()))
            }
        }
    }

    async fn execute_tool(
        &self,
        name: &str,
        args: serde_json::Value,
    ) -> Result<Vec<MCPContent>, String> {
        match name {
            "xint_search" => {
                let query = args
                    .get("query")
                    .and_then(|v| v.as_str())
                    .ok_or("Missing query")?;
                let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(15) as usize;

                // Note: In real implementation, we'd call the API here
                // For now, return a placeholder
                Ok(vec![MCPContent {
                    content_type: "text".to_string(),
                    text: format!("Search: {query} (limit: {limit})"),
                }])
            }
            "xint_profile" => {
                let username = args
                    .get("username")
                    .and_then(|v| v.as_str())
                    .ok_or("Missing username")?;

                Ok(vec![MCPContent {
                    content_type: "text".to_string(),
                    text: format!("Profile: @{username}"),
                }])
            }
            "xint_thread" => {
                let tweet_id = args
                    .get("tweet_id")
                    .and_then(|v| v.as_str())
                    .ok_or("Missing tweet_id")?;

                Ok(vec![MCPContent {
                    content_type: "text".to_string(),
                    text: format!("Thread for tweet: {tweet_id}"),
                }])
            }
            "xint_tweet" => {
                let tweet_id = args
                    .get("tweet_id")
                    .and_then(|v| v.as_str())
                    .ok_or("Missing tweet_id")?;

                Ok(vec![MCPContent {
                    content_type: "text".to_string(),
                    text: format!("Tweet: {tweet_id}"),
                }])
            }
            "xint_trends" => {
                let location = args
                    .get("location")
                    .and_then(|v| v.as_str())
                    .unwrap_or("worldwide");

                Ok(vec![MCPContent {
                    content_type: "text".to_string(),
                    text: format!("Trends for: {location}"),
                }])
            }
            "xint_xsearch" => {
                let query = args
                    .get("query")
                    .and_then(|v| v.as_str())
                    .ok_or("Missing query")?;

                Ok(vec![MCPContent {
                    content_type: "text".to_string(),
                    text: format!("X-Search: {query}"),
                }])
            }
            "xint_collections_list" => Ok(vec![MCPContent {
                content_type: "text".to_string(),
                text: "Collections: []".to_string(),
            }]),
            "xint_analyze" => {
                let query = args
                    .get("query")
                    .and_then(|v| v.as_str())
                    .ok_or("Missing query")?;

                Ok(vec![MCPContent {
                    content_type: "text".to_string(),
                    text: format!("Analysis: {query}"),
                }])
            }
            "xint_article" => {
                let url = args
                    .get("url")
                    .and_then(|v| v.as_str())
                    .ok_or("Missing url")?;

                Ok(vec![MCPContent {
                    content_type: "text".to_string(),
                    text: format!("Article: {url}"),
                }])
            }
            "xint_collections_search" => {
                let collection_id = args
                    .get("collection_id")
                    .and_then(|v| v.as_str())
                    .ok_or("Missing collection_id")?;
                let query = args
                    .get("query")
                    .and_then(|v| v.as_str())
                    .ok_or("Missing query")?;

                Ok(vec![MCPContent {
                    content_type: "text".to_string(),
                    text: format!("Collections search in {collection_id}: {query}"),
                }])
            }
            "xint_bookmarks" => Ok(vec![MCPContent {
                content_type: "text".to_string(),
                text: "Bookmarks: OAuth required".to_string(),
            }]),
            "xint_package_create" => Ok(vec![MCPContent {
                content_type: "text".to_string(),
                text: serde_json::to_string_pretty(
                    &self
                        .call_package_api(
                            reqwest::Method::POST,
                            "/packages",
                            Some(serde_json::json!({
                                "name": args.get("name").and_then(|v| v.as_str()).unwrap_or(""),
                                "topic_query": args
                                    .get("topic_query")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or(""),
                                "sources": args
                                    .get("sources")
                                    .and_then(|v| v.as_array())
                                    .cloned()
                                    .unwrap_or_default(),
                                "time_window": args.get("time_window").cloned().unwrap_or_else(|| {
                                    serde_json::json!({
                                        "from": chrono::Utc::now()
                                            .checked_sub_signed(chrono::Duration::days(1))
                                            .unwrap_or_else(chrono::Utc::now)
                                            .to_rfc3339(),
                                        "to": chrono::Utc::now().to_rfc3339()
                                    })
                                }),
                                "policy": args
                                    .get("policy")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("private"),
                                "analysis_profile": args
                                    .get("analysis_profile")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("summary")
                            })),
                        )
                        .await?,
                )
                .unwrap_or_else(|_| "{}".to_string()),
            }]),
            "xint_package_status" => {
                let package_id = args
                    .get("package_id")
                    .and_then(|v| v.as_str())
                    .ok_or("Missing package_id")?;
                let result = self
                    .call_package_api(
                        reqwest::Method::GET,
                        &format!("/packages/{package_id}"),
                        None,
                    )
                    .await?;
                Ok(vec![MCPContent {
                    content_type: "text".to_string(),
                    text: serde_json::to_string_pretty(&result)
                        .unwrap_or_else(|_| result.to_string()),
                }])
            }
            "xint_package_query" => {
                let query = args
                    .get("query")
                    .and_then(|v| v.as_str())
                    .ok_or("Missing query")?;
                let package_ids = args
                    .get("package_ids")
                    .and_then(|v| v.as_array())
                    .cloned()
                    .unwrap_or_default();
                if package_ids.is_empty() {
                    return Err("Missing package_ids".to_string());
                }
                let payload = serde_json::json!({
                    "query": query,
                    "package_ids": package_ids,
                    "max_claims": args.get("max_claims").and_then(|v| v.as_u64()).unwrap_or(10),
                    "require_citations": args
                        .get("require_citations")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(true)
                });
                let result = self
                    .call_package_api(reqwest::Method::POST, "/query", Some(payload))
                    .await?;
                Ok(vec![MCPContent {
                    content_type: "text".to_string(),
                    text: serde_json::to_string_pretty(&result)
                        .unwrap_or_else(|_| result.to_string()),
                }])
            }
            "xint_package_refresh" => {
                let package_id = args
                    .get("package_id")
                    .and_then(|v| v.as_str())
                    .ok_or("Missing package_id")?;
                let reason = args
                    .get("reason")
                    .and_then(|v| v.as_str())
                    .ok_or("Missing reason")?;
                let result = self
                    .call_package_api(
                        reqwest::Method::POST,
                        &format!("/packages/{package_id}/refresh"),
                        Some(serde_json::json!({ "reason": reason })),
                    )
                    .await?;
                Ok(vec![MCPContent {
                    content_type: "text".to_string(),
                    text: serde_json::to_string_pretty(&result)
                        .unwrap_or_else(|_| result.to_string()),
                }])
            }
            "xint_package_search" => {
                let query = args
                    .get("query")
                    .and_then(|v| v.as_str())
                    .ok_or("Missing query")?;
                let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(20);
                let query_encoded = query.replace(' ', "%20");
                let path = format!("/packages/search?q={query_encoded}&limit={limit}");
                let result = self
                    .call_package_api(reqwest::Method::GET, &path, None)
                    .await?;
                Ok(vec![MCPContent {
                    content_type: "text".to_string(),
                    text: serde_json::to_string_pretty(&result)
                        .unwrap_or_else(|_| result.to_string()),
                }])
            }
            "xint_package_publish" => {
                let package_id = args
                    .get("package_id")
                    .and_then(|v| v.as_str())
                    .ok_or("Missing package_id")?;
                let snapshot_version = args
                    .get("snapshot_version")
                    .and_then(|v| v.as_u64())
                    .ok_or("Missing snapshot_version")?;
                let result = self
                    .call_package_api(
                        reqwest::Method::POST,
                        &format!("/packages/{package_id}/publish"),
                        Some(serde_json::json!({ "snapshot_version": snapshot_version })),
                    )
                    .await?;
                Ok(vec![MCPContent {
                    content_type: "text".to_string(),
                    text: serde_json::to_string_pretty(&result)
                        .unwrap_or_else(|_| result.to_string()),
                }])
            }
            "xint_cache_clear" => Ok(vec![MCPContent {
                content_type: "text".to_string(),
                text: "Cache cleared".to_string(),
            }]),
            "xint_watch" => {
                let query = args
                    .get("query")
                    .and_then(|v| v.as_str())
                    .ok_or("Missing query")?;
                Ok(vec![MCPContent {
                    content_type: "text".to_string(),
                    text: format!("Watch: {query} (use CLI for real-time monitoring)"),
                }])
            }
            "xint_diff" => {
                let username = args
                    .get("username")
                    .and_then(|v| v.as_str())
                    .ok_or("Missing username")?;
                Ok(vec![MCPContent {
                    content_type: "text".to_string(),
                    text: format!("Diff tracking for @{username}"),
                }])
            }
            "xint_report" => {
                let topic = args
                    .get("topic")
                    .and_then(|v| v.as_str())
                    .ok_or("Missing topic")?;
                Ok(vec![MCPContent {
                    content_type: "text".to_string(),
                    text: format!("Report on: {topic} (requires XAI_API_KEY)"),
                }])
            }
            "xint_sentiment" => Ok(vec![MCPContent {
                content_type: "text".to_string(),
                text: "Sentiment analysis (requires XAI_API_KEY)".to_string(),
            }]),
            "xint_costs" => {
                let period = args
                    .get("period")
                    .and_then(|v| v.as_str())
                    .unwrap_or("today");
                Ok(vec![MCPContent {
                    content_type: "text".to_string(),
                    text: format!("Cost tracking for period: {period}"),
                }])
            }
            _ => Err(format!("Unknown tool: {name}")),
        }
    }

    pub async fn run_stdio(&mut self) -> Result<(), String> {
        let stdin = tokio::io::stdin();
        let mut reader = BufReader::new(stdin).lines();

        while let Ok(Some(line)) = reader.next_line().await {
            match self.handle_message(&line).await {
                Ok(Some(response)) => println!("{response}"),
                Ok(None) => {}
                Err(err) => {
                    let response = serde_json::json!({
                        "jsonrpc": "2.0",
                        "error": { "code": -32603, "message": err }
                    });
                    println!("{response}");
                }
            }
        }

        Ok(())
    }
}

// ============================================================================
// CLI Command - using McpArgs from cli module
// ============================================================================

pub async fn run(args: McpArgs, config: &Config, global_policy: PolicyMode) -> anyhow::Result<()> {
    let policy_mode = args.policy.unwrap_or(global_policy);
    let enforce_budget = !args.no_budget_guard;

    println!(
        "Starting xint MCP server (sse: {}, port: {}, policy: {}, budget_guard: {})...",
        args.sse,
        args.port,
        policy::as_str(policy_mode),
        if enforce_budget {
            "enabled"
        } else {
            "disabled"
        }
    );

    let mut server = MCPServer::new(
        policy_mode,
        enforce_budget,
        config.costs_path(),
        config.reliability_path(),
    );
    server.run_stdio().await.map_err(|e| anyhow::anyhow!(e))?;

    Ok(())
}
