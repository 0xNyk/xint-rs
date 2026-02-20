//! xint MCP Server
//!
//! MCP (Model Context Protocol) server implementation for xint-rs CLI.
//! Exposes xint functionality as MCP tools for AI agents like Claude Code.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tokio::io::{AsyncBufReadExt, BufReader};

use crate::api::{grok, twitter, xai};
use crate::auth::oauth;
use crate::cache;
use crate::cli::{McpArgs, PolicyMode};
use crate::client::XClient;
use crate::config::Config;
use crate::costs;
use crate::mcp_dispatcher::{resolve_tool_route, McpToolRoute};
use crate::models::Tweet;
use crate::policy;
use crate::reliability;
use crate::sentiment;

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
#[allow(dead_code)]
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

    fn package_api_workspace_id() -> Option<String> {
        std::env::var("XINT_WORKSPACE_ID")
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    }

    fn billing_upgrade_url() -> String {
        std::env::var("XINT_BILLING_UPGRADE_URL")
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "https://xint.dev/pricing".to_string())
    }

    async fn call_package_api(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<serde_json::Value>,
    ) -> Result<serde_json::Value, String> {
        let base = Self::package_api_base_url().ok_or_else(|| {
            "XINT_PACKAGE_API_BASE_URL not set. Start xint-cloud service on :8787 and set XINT_PACKAGE_API_BASE_URL=http://localhost:8787/v1".to_string()
        })?;
        let url = format!("{}{}", base.trim_end_matches('/'), path);

        let client = reqwest::Client::new();
        let mut req = client.request(method, &url);
        if let Some(key) = Self::package_api_key() {
            req = req.header(reqwest::header::AUTHORIZATION, format!("Bearer {key}"));
        }
        if let Some(workspace_id) = Self::package_api_workspace_id() {
            req = req.header("x-workspace-id", workspace_id);
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
            if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&text) {
                let code = parsed
                    .get("code")
                    .and_then(|v| v.as_str())
                    .unwrap_or("UNKNOWN");
                let error_msg = parsed
                    .get("error")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Package API request failed");
                let mut message =
                    format!("Package API {} [{}]: {}", status.as_u16(), code, error_msg);
                if matches!(
                    code,
                    "PLAN_REQUIRED" | "QUOTA_EXCEEDED" | "FEATURE_NOT_IN_PLAN"
                ) {
                    message = format!("{message}. Upgrade: {}", Self::billing_upgrade_url());
                }
                return Err(message);
            }
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

    fn ensure_package_query_citations(
        &self,
        result: &serde_json::Value,
        require_citations: bool,
    ) -> Result<(), String> {
        if !require_citations {
            return Ok(());
        }
        let obj = result
            .as_object()
            .ok_or_else(|| "Package API query response must be a JSON object.".to_string())?;

        let claims = obj
            .get("claims")
            .and_then(serde_json::Value::as_array)
            .cloned()
            .unwrap_or_default();
        let citations = obj
            .get("citations")
            .and_then(serde_json::Value::as_array)
            .cloned()
            .unwrap_or_default();

        if !claims.is_empty() && citations.is_empty() {
            return Err(
                "Package API query response missing citations while require_citations=true."
                    .to_string(),
            );
        }

        let claim_ids: std::collections::HashSet<String> = claims
            .iter()
            .filter_map(|claim| {
                claim
                    .as_object()
                    .and_then(|item| item.get("claim_id"))
                    .and_then(serde_json::Value::as_str)
                    .map(ToOwned::to_owned)
            })
            .collect();

        if claim_ids.is_empty() {
            return Ok(());
        }

        let cited_claim_ids: std::collections::HashSet<String> = citations
            .iter()
            .filter_map(|citation| {
                let obj = citation.as_object()?;
                let claim_id = obj.get("claim_id")?.as_str()?;
                let url = obj.get("url")?.as_str()?;
                if claim_id.is_empty() || url.is_empty() {
                    return None;
                }
                Some(claim_id.to_string())
            })
            .collect();

        for claim_id in claim_ids {
            if !cited_claim_ids.contains(&claim_id) {
                return Err(format!(
                    "Package API query response has uncited claim '{claim_id}' while require_citations=true."
                ));
            }
        }
        Ok(())
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
                        let command_name = format!("mcp:{name}");
                        reliability::record_command_result(
                            &self.reliability_path,
                            &command_name,
                            true,
                            started_at.elapsed().as_millis(),
                            reliability::ReliabilityMode::Mcp,
                            reliability::consume_command_fallback(&command_name),
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
        fn make_content(text: String) -> Vec<MCPContent> {
            vec![MCPContent {
                content_type: "text".to_string(),
                text,
            }]
        }

        fn json_content(payload: serde_json::Value) -> Result<Vec<MCPContent>, String> {
            serde_json::to_string_pretty(&payload)
                .map(make_content)
                .map_err(|e| format!("Failed to encode MCP response payload: {e}"))
        }

        fn bool_arg(args: &serde_json::Value, camel: &str, snake: &str) -> bool {
            args.get(camel)
                .and_then(|v| v.as_bool())
                .or_else(|| args.get(snake).and_then(|v| v.as_bool()))
                .unwrap_or(false)
        }

        fn extract_tweet_id(input: &str) -> String {
            if let Some(idx) = input.find("/status/") {
                let suffix = &input[(idx + "/status/".len())..];
                let id: String = suffix.chars().take_while(|c| c.is_ascii_digit()).collect();
                if !id.is_empty() {
                    return id;
                }
            }
            input.to_string()
        }

        fn resolve_woeid(location: &str) -> Result<u32, String> {
            let trimmed = location.trim();
            if let Ok(woeid) = trimmed.parse::<u32>() {
                return Ok(woeid);
            }

            let key = trimmed.to_lowercase();
            let woeid = match key.as_str() {
                "worldwide" | "world" | "global" => Some(1),
                "us" | "usa" | "united states" => Some(23424977),
                "uk" | "united kingdom" => Some(23424975),
                "canada" => Some(23424775),
                "australia" => Some(23424748),
                "india" => Some(23424848),
                "japan" => Some(23424856),
                "germany" => Some(23424829),
                "france" => Some(23424819),
                "brazil" => Some(23424768),
                "mexico" => Some(23424900),
                "spain" => Some(23424950),
                "italy" => Some(23424853),
                "netherlands" => Some(23424909),
                "south korea" | "korea" => Some(23424868),
                "turkey" => Some(23424969),
                "indonesia" => Some(23424846),
                "nigeria" => Some(23424908),
                "south africa" => Some(23424942),
                "singapore" => Some(23424948),
                "new zealand" => Some(23424916),
                "argentina" => Some(23424747),
                "colombia" => Some(23424787),
                "philippines" => Some(23424934),
                "egypt" => Some(23424802),
                "israel" => Some(23424852),
                "ireland" => Some(23424803),
                "sweden" => Some(23424954),
                "poland" => Some(23424923),
                _ => None,
            };

            woeid.ok_or_else(|| {
                format!(
                    "Unknown location: \"{location}\". Use a known location name or numeric WOEID."
                )
            })
        }

        fn language_for_woeid(woeid: u32) -> &'static str {
            match woeid {
                23424856 => "ja",
                23424829 => "de",
                23424819 => "fr",
                23424768 => "pt",
                23424900 | 23424950 | 23424747 | 23424787 => "es",
                23424853 => "it",
                23424868 => "ko",
                23424969 => "tr",
                23424846 => "id",
                23424923 => "pl",
                23424954 => "sv",
                _ => "en",
            }
        }

        fn woeid_name(woeid: u32) -> &'static str {
            match woeid {
                1 => "Worldwide",
                23424977 => "United States",
                23424975 => "United Kingdom",
                23424775 => "Canada",
                23424748 => "Australia",
                23424848 => "India",
                23424856 => "Japan",
                23424829 => "Germany",
                23424819 => "France",
                23424768 => "Brazil",
                23424900 => "Mexico",
                23424950 => "Spain",
                23424853 => "Italy",
                23424909 => "Netherlands",
                23424868 => "South Korea",
                23424969 => "Turkey",
                23424846 => "Indonesia",
                23424908 => "Nigeria",
                23424942 => "South Africa",
                23424948 => "Singapore",
                23424916 => "New Zealand",
                23424747 => "Argentina",
                23424787 => "Colombia",
                23424934 => "Philippines",
                23424802 => "Egypt",
                23424852 => "Israel",
                23424803 => "Ireland",
                23424954 => "Sweden",
                23424923 => "Poland",
                _ => "Unknown",
            }
        }

        fn bearer_runtime() -> Result<(XClient, String), String> {
            let runtime_config =
                Config::load().map_err(|e| format!("Failed to load config: {e}"))?;
            let token = runtime_config
                .require_bearer_token()
                .map_err(|e| e.to_string())?
                .to_string();
            let client =
                XClient::new().map_err(|e| format!("Failed to initialize HTTP client: {e}"))?;
            Ok((client, token))
        }

        fn xai_runtime() -> Result<String, String> {
            let runtime_config =
                Config::load().map_err(|e| format!("Failed to load config: {e}"))?;
            runtime_config
                .require_xai_key()
                .map(|v| v.to_string())
                .map_err(|e| e.to_string())
        }

        fn xai_management_runtime() -> Result<String, String> {
            let runtime_config =
                Config::load().map_err(|e| format!("Failed to load config: {e}"))?;
            runtime_config
                .require_xai_management_key()
                .map(|v| v.to_string())
                .map_err(|e| e.to_string())
        }

        async fn oauth_runtime() -> Result<(XClient, String), String> {
            let runtime_config =
                Config::load().map_err(|e| format!("Failed to load config: {e}"))?;
            let client =
                XClient::new().map_err(|e| format!("Failed to initialize HTTP client: {e}"))?;
            let client_id = runtime_config
                .require_client_id()
                .map_err(|e| e.to_string())?
                .to_string();
            let (token, _) =
                oauth::get_valid_token(&client, &runtime_config.tokens_path(), &client_id)
                    .await
                    .map_err(|e| e.to_string())?;
            Ok((client, token))
        }

        let route_result = resolve_tool_route(name);
        let Some(route) = route_result.data else {
            return Err(route_result.message);
        };
        match route {
            McpToolRoute::Search => {
                let (client, token) = bearer_runtime()?;
                let query = args
                    .get("query")
                    .and_then(|v| v.as_str())
                    .ok_or("Missing query")?;
                let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(15) as usize;
                let pages = ((limit.max(1) as u32).saturating_add(19) / 20).min(5);
                let sort_order = match args.get("sort").and_then(|v| v.as_str()).unwrap_or("likes")
                {
                    "recent" | "recency" => "recency",
                    _ => "relevancy",
                };
                let since = args.get("since").and_then(|v| v.as_str());
                let no_retweets = bool_arg(&args, "noRetweets", "no_retweets");
                let no_replies = bool_arg(&args, "noReplies", "no_replies");

                let mut tweets = twitter::search(
                    &client, &token, query, pages, sort_order, since, None, false,
                )
                .await
                .map_err(|e| format!("Search failed: {e}"))?;

                if no_retweets {
                    tweets.retain(|t| !t.text.starts_with("RT @"));
                }
                if no_replies {
                    tweets.retain(|t| t.conversation_id == t.id);
                }
                let shown: Vec<_> = tweets.into_iter().take(limit.max(1)).collect();
                costs::track_cost(
                    &self.costs_path,
                    "search",
                    "/2/tweets/search/recent",
                    shown.len() as u64,
                );

                json_content(serde_json::json!({
                    "type": "success",
                    "message": "Search completed.",
                    "data": shown
                }))
            }
            McpToolRoute::Profile => {
                let (client, token) = bearer_runtime()?;
                let username = args
                    .get("username")
                    .and_then(|v| v.as_str())
                    .ok_or("Missing username")?;
                let count = args.get("count").and_then(|v| v.as_u64()).unwrap_or(20) as u32;
                let include_replies = bool_arg(&args, "includeReplies", "include_replies");
                let normalized = username.trim_start_matches('@');

                let (user, tweets) =
                    twitter::get_profile(&client, &token, normalized, count, include_replies)
                        .await
                        .map_err(|e| format!("Profile lookup failed: {e}"))?;
                costs::track_cost(
                    &self.costs_path,
                    "profile",
                    &format!("/2/users/by/username/{normalized}"),
                    tweets.len() as u64 + 1,
                );

                json_content(serde_json::json!({
                    "type": "success",
                    "message": "Profile lookup completed.",
                    "data": {
                        "user": user,
                        "tweets": tweets
                    }
                }))
            }
            McpToolRoute::Thread => {
                let (client, token) = bearer_runtime()?;
                let tweet_id = args
                    .get("tweet_id")
                    .or_else(|| args.get("tweetId"))
                    .and_then(|v| v.as_str())
                    .ok_or("Missing tweet_id or tweetId")?;
                let pages = args.get("pages").and_then(|v| v.as_u64()).unwrap_or(2) as u32;
                let normalized_id = extract_tweet_id(tweet_id);

                let tweets = twitter::get_thread(&client, &token, &normalized_id, pages)
                    .await
                    .map_err(|e| format!("Thread lookup failed: {e}"))?;
                costs::track_cost(
                    &self.costs_path,
                    "thread",
                    "/2/tweets/search/recent",
                    tweets.len() as u64,
                );

                json_content(serde_json::json!({
                    "type": "success",
                    "message": "Thread lookup completed.",
                    "data": {
                        "tweets": tweets
                    }
                }))
            }
            McpToolRoute::Tweet => {
                let (client, token) = bearer_runtime()?;
                let tweet_id = args
                    .get("tweet_id")
                    .or_else(|| args.get("tweetId"))
                    .and_then(|v| v.as_str())
                    .ok_or("Missing tweet_id or tweetId")?;
                let normalized_id = extract_tweet_id(tweet_id);
                let tweet = twitter::get_tweet(&client, &token, &normalized_id)
                    .await
                    .map_err(|e| format!("Tweet lookup failed: {e}"))?;
                costs::track_cost(
                    &self.costs_path,
                    "tweet",
                    &format!("/2/tweets/{normalized_id}"),
                    if tweet.is_some() { 1 } else { 0 },
                );

                json_content(serde_json::json!({
                    "type": "success",
                    "message": "Tweet lookup completed.",
                    "data": tweet
                }))
            }
            McpToolRoute::Trends => {
                let (client, token) = bearer_runtime()?;
                let location = args
                    .get("location")
                    .and_then(|v| v.as_str())
                    .unwrap_or("worldwide");
                let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(20) as usize;
                let woeid = resolve_woeid(location)?;
                let trends_path = format!("trends/by/woeid/{woeid}");

                let primary = client.bearer_get(&trends_path, &token).await.ok().and_then(|raw| {
                    let data = raw.data?;
                    let arr = data.as_array()?;
                    let trends: Vec<serde_json::Value> = arr
                        .iter()
                        .filter_map(|item| {
                            let name = item.get("trend_name")?.as_str()?;
                            Some(serde_json::json!({
                                "name": name,
                                "tweet_count": item.get("tweet_count").and_then(|v| v.as_u64()),
                                "url": format!("https://x.com/search?q={}", name.replace(' ', "%20")),
                                "category": item.get("category").and_then(|v| v.as_str()),
                            }))
                        })
                        .collect();

                    if trends.is_empty() {
                        None
                    } else {
                        Some(serde_json::json!({
                            "source": "api",
                            "location": woeid_name(woeid),
                            "woeid": woeid,
                            "trends": trends.into_iter().take(limit.max(1)).collect::<Vec<_>>(),
                            "fetched_at": chrono::Utc::now().to_rfc3339()
                        }))
                    }
                });

                if let Some(result) = primary {
                    costs::track_cost(&self.costs_path, "trends", "/2/trends/by/woeid", 0);
                    return json_content(serde_json::json!({
                        "type": "success",
                        "message": "Trends fetch completed.",
                        "data": result
                    }));
                }

                let fallback_query = format!("-is:retweet lang:{}", language_for_woeid(woeid));
                let fallback_tweets = twitter::search(
                    &client,
                    &token,
                    &fallback_query,
                    1,
                    "recency",
                    None,
                    None,
                    false,
                )
                .await
                .map_err(|e| format!("Trends fallback failed: {e}"))?;
                costs::track_cost(
                    &self.costs_path,
                    "search",
                    "/2/tweets/search/recent",
                    fallback_tweets.len() as u64,
                );

                let mut counts = std::collections::HashMap::<String, u64>::new();
                for tweet in &fallback_tweets {
                    for hashtag in &tweet.hashtags {
                        let tag = format!("#{}", hashtag.to_lowercase());
                        *counts.entry(tag).or_insert(0) += 1;
                    }
                    for word in tweet.text.split_whitespace() {
                        if word.starts_with('#') && word.len() > 1 {
                            let tag = word.to_lowercase();
                            *counts.entry(tag).or_insert(0) += 1;
                        }
                        if word.starts_with('$')
                            && word.len() > 1
                            && word[1..].chars().all(|c| c.is_ascii_alphabetic())
                        {
                            let tag = word.to_uppercase();
                            *counts.entry(tag).or_insert(0) += 1;
                        }
                    }
                }

                let mut sorted: Vec<(String, u64)> = counts
                    .into_iter()
                    .filter(|(_, count)| *count >= 2)
                    .collect();
                sorted.sort_by(|a, b| b.1.cmp(&a.1));

                let trends: Vec<serde_json::Value> = sorted
                    .into_iter()
                    .take(limit.max(1))
                    .map(|(name, count)| {
                        serde_json::json!({
                            "name": name,
                            "tweet_count": count,
                            "url": format!("https://x.com/search?q={}", name.replace(' ', "%20"))
                        })
                    })
                    .collect();

                reliability::mark_command_fallback("mcp:xint_trends");
                json_content(serde_json::json!({
                    "type": "success",
                    "message": "Trends fetch completed via fallback.",
                    "data": {
                        "source": "search_fallback",
                        "location": woeid_name(woeid),
                        "woeid": woeid,
                        "trends": trends,
                        "fetched_at": chrono::Utc::now().to_rfc3339()
                    }
                }))
            }
            McpToolRoute::XSearch => {
                let api_key = xai_runtime()?;
                let query = args
                    .get("query")
                    .and_then(|v| v.as_str())
                    .ok_or("Missing query")?;
                let max_results = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(10) as u32;
                let model = args
                    .get("model")
                    .and_then(|v| v.as_str())
                    .unwrap_or("grok-4");
                let http = reqwest::Client::new();
                let (results, summary) =
                    xai::x_search(&http, &api_key, query, max_results, None, None, model, 45)
                        .await
                        .map_err(|e| format!("x_search failed: {e}"))?;

                json_content(serde_json::json!({
                    "type": "success",
                    "message": "x_search completed.",
                    "data": {
                        "query": query,
                        "model": model,
                        "result_count": results.len(),
                        "summary": summary,
                        "results": results
                    }
                }))
            }
            McpToolRoute::CollectionsList => Ok(vec![MCPContent {
                content_type: "text".to_string(),
                text: serde_json::to_string_pretty(&{
                    let result =
                        xai::collections_list(&reqwest::Client::new(), &xai_management_runtime()?)
                            .await
                            .map_err(|e| format!("collections list failed: {e}"))?;
                    serde_json::json!({
                        "type": "success",
                        "message": "Collections list fetched.",
                        "data": result
                    })
                })
                .map_err(|e| format!("Failed to encode collections list payload: {e}"))?,
            }]),
            McpToolRoute::Analyze => {
                let api_key = xai_runtime()?;
                let query = args
                    .get("query")
                    .and_then(|v| v.as_str())
                    .ok_or("Missing query")?;
                let model = args
                    .get("model")
                    .and_then(|v| v.as_str())
                    .unwrap_or("grok-3-mini")
                    .to_string();
                let opts = crate::models::GrokOpts {
                    model: model.clone(),
                    ..Default::default()
                };
                let http = reqwest::Client::new();

                let response = if let Some(tweets_raw) = args.get("tweets") {
                    let tweets: Vec<Tweet> = serde_json::from_value(tweets_raw.clone())
                        .map_err(|e| format!("Invalid tweets payload for analyze: {e}"))?;
                    grok::analyze_tweets(&http, &api_key, &tweets, Some(query), &opts)
                        .await
                        .map_err(|e| format!("Analyze tweets failed: {e}"))?
                } else {
                    grok::analyze_query(&http, &api_key, query, None, &opts)
                        .await
                        .map_err(|e| format!("Analyze query failed: {e}"))?
                };

                json_content(serde_json::json!({
                    "type": "success",
                    "message": "Analysis completed.",
                    "data": {
                        "model": response.model,
                        "content": response.content,
                        "usage": response.usage
                    }
                }))
            }
            McpToolRoute::Article => {
                let url = args
                    .get("url")
                    .and_then(|v| v.as_str())
                    .ok_or("Missing url")?;
                json_content(serde_json::json!({
                    "type": "success",
                    "message": "Article route acknowledged.",
                    "data": {
                        "url": url,
                        "note": "Use CLI article command for full web/article extraction output."
                    }
                }))
            }
            McpToolRoute::CollectionsSearch => {
                let api_key = xai_runtime()?;
                let collection_id = args
                    .get("collection_id")
                    .or_else(|| args.get("collectionId"))
                    .and_then(|v| v.as_str())
                    .ok_or("Missing collection_id or collectionId")?;
                let query = args
                    .get("query")
                    .and_then(|v| v.as_str())
                    .ok_or("Missing query")?;
                let top_k = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(8) as u32;
                let http = reqwest::Client::new();
                let result = xai::documents_search(
                    &http,
                    &api_key,
                    &[collection_id.to_string()],
                    query,
                    top_k,
                )
                .await
                .map_err(|e| format!("Collections search failed: {e}"))?;

                json_content(serde_json::json!({
                    "type": "success",
                    "message": "Collections search completed.",
                    "data": result
                }))
            }
            McpToolRoute::Bookmarks => json_content(serde_json::json!({
                "type": "info",
                "message": "Bookmarks requires OAuth user context.",
                "data": {
                    "note": "Use xint bookmarks command for full OAuth flow and caching behavior."
                }
            })),
            McpToolRoute::PackageCreate => {
                let payload = serde_json::json!({
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
                });
                let result = self
                    .call_package_api(reqwest::Method::POST, "/packages", Some(payload))
                    .await?;
                json_content(serde_json::json!({
                    "type": "success",
                    "message": "Package create request accepted.",
                    "data": result
                }))
            }
            McpToolRoute::PackageStatus => {
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
                json_content(serde_json::json!({
                    "type": "success",
                    "message": "Package status fetched.",
                    "data": result
                }))
            }
            McpToolRoute::PackageQuery => {
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
                let require_citations = args
                    .get("require_citations")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(true);
                let payload = serde_json::json!({
                    "query": query,
                    "package_ids": package_ids,
                    "max_claims": args.get("max_claims").and_then(|v| v.as_u64()).unwrap_or(10),
                    "require_citations": require_citations
                });
                let result = self
                    .call_package_api(reqwest::Method::POST, "/query", Some(payload))
                    .await?;
                self.ensure_package_query_citations(&result, require_citations)?;
                json_content(serde_json::json!({
                    "type": "success",
                    "message": "Package query completed.",
                    "data": result
                }))
            }
            McpToolRoute::PackageRefresh => {
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
                json_content(serde_json::json!({
                    "type": "success",
                    "message": "Package refresh requested.",
                    "data": result
                }))
            }
            McpToolRoute::PackageSearch => {
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
                json_content(serde_json::json!({
                    "type": "success",
                    "message": "Package search completed.",
                    "data": result
                }))
            }
            McpToolRoute::PackagePublish => {
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
                json_content(serde_json::json!({
                    "type": "success",
                    "message": "Package publish requested.",
                    "data": result
                }))
            }
            McpToolRoute::CacheClear => {
                let runtime_config =
                    Config::load().map_err(|e| format!("Failed to load config: {e}"))?;
                let cleared = cache::clear(&runtime_config.cache_dir());
                json_content(serde_json::json!({
                    "type": "success",
                    "message": "Cache cleared.",
                    "data": {
                        "cleared": cleared
                    }
                }))
            }
            McpToolRoute::Watch => {
                let (client, token) = bearer_runtime()?;
                let query = args
                    .get("query")
                    .and_then(|v| v.as_str())
                    .ok_or("Missing query")?;
                let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(10) as usize;
                let since = args.get("since").and_then(|v| v.as_str()).unwrap_or("1h");
                let search_query = if query.starts_with('@') && !query.contains(' ') {
                    format!("from:{} -is:retweet", query.trim_start_matches('@'))
                } else if query.contains("is:retweet") {
                    query.to_string()
                } else {
                    format!("{query} -is:retweet")
                };

                let tweets = twitter::search(
                    &client,
                    &token,
                    &search_query,
                    1,
                    "recency",
                    Some(since),
                    None,
                    false,
                )
                .await
                .map_err(|e| format!("Watch probe failed: {e}"))?;
                let shown: Vec<_> = tweets.into_iter().take(limit.max(1)).collect();
                costs::track_cost(
                    &self.costs_path,
                    "search",
                    "/2/tweets/search/recent",
                    shown.len() as u64,
                );

                json_content(serde_json::json!({
                    "type": "success",
                    "message": "Watch probe completed.",
                    "data": {
                        "query": search_query,
                        "since": since,
                        "continuous_mode": false,
                        "hint": "Use CLI watch command for continuous polling and webhooks.",
                        "tweets": shown
                    }
                }))
            }
            McpToolRoute::Diff => {
                let (client, access_token) = oauth_runtime().await?;
                let username = args
                    .get("username")
                    .and_then(|v| v.as_str())
                    .ok_or("Missing username")?;
                let normalized = username.trim_start_matches('@');
                let following = bool_arg(&args, "following", "following");
                let snap_type = if following { "following" } else { "followers" };
                let pages = args.get("pages").and_then(|v| v.as_u64()).unwrap_or(5) as u32;

                let lookup_path =
                    format!("users/by/username/{normalized}?user.fields=public_metrics");
                let lookup = client
                    .oauth_get(&lookup_path, &access_token)
                    .await
                    .map_err(|e| format!("Failed to resolve @{normalized}: {e}"))?;
                let user_id = lookup
                    .data
                    .as_ref()
                    .and_then(|d| d.get("id"))
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| format!("User @{normalized} not found"))?
                    .to_string();

                let mut users = Vec::<serde_json::Value>::new();
                let mut next_token: Option<String> = None;
                for page_idx in 0..pages {
                    let pagination = match &next_token {
                        Some(t) => format!("&pagination_token={t}"),
                        None => String::new(),
                    };
                    let path = format!(
                        "users/{user_id}/{snap_type}?max_results=1000&user.fields=public_metrics,username,name{pagination}"
                    );
                    let raw = client
                        .oauth_get(&path, &access_token)
                        .await
                        .map_err(|e| format!("Failed to fetch {snap_type}: {e}"))?;
                    if let Some(data) = &raw.data {
                        if let Some(arr) = data.as_array() {
                            users.extend(arr.iter().cloned());
                        }
                    }
                    next_token = raw.meta.and_then(|m| m.next_token);
                    if next_token.is_none() {
                        break;
                    }
                    if page_idx + 1 < pages {
                        crate::client::rate_delay().await;
                    }
                }

                costs::track_cost(
                    &self.costs_path,
                    snap_type,
                    &format!("/2/users/{user_id}/{snap_type}"),
                    users.len() as u64,
                );

                json_content(serde_json::json!({
                    "type": "success",
                    "message": "Diff snapshot fetched.",
                    "data": {
                        "username": normalized,
                        "mode": snap_type,
                        "count": users.len(),
                        "users": users,
                        "note": "Use CLI diff command for snapshot history and delta computation."
                    }
                }))
            }
            McpToolRoute::Report => {
                let (client, token) = bearer_runtime()?;
                let topic = args
                    .get("topic")
                    .or_else(|| args.get("query"))
                    .and_then(|v| v.as_str())
                    .ok_or("Missing topic or query")?;
                let pages = args.get("pages").and_then(|v| v.as_u64()).unwrap_or(2) as u32;
                let tweets = twitter::search(
                    &client,
                    &token,
                    topic,
                    pages.min(5),
                    "relevancy",
                    Some("1d"),
                    None,
                    false,
                )
                .await
                .map_err(|e| format!("Report search failed: {e}"))?;

                costs::track_cost(
                    &self.costs_path,
                    "search",
                    "/2/tweets/search/recent",
                    tweets.len() as u64,
                );

                let mut top_tweets = tweets.clone();
                twitter::sort_by(&mut top_tweets, "likes");
                top_tweets.truncate(10);

                json_content(serde_json::json!({
                    "type": "success",
                    "message": "Report data prepared.",
                    "data": {
                        "topic": topic,
                        "generated_at": chrono::Utc::now().to_rfc3339(),
                        "tweet_count": tweets.len(),
                        "top_tweets": top_tweets,
                        "ai_summary": serde_json::Value::Null,
                        "note": "MCP report currently returns source data only; use CLI report for AI narrative output."
                    }
                }))
            }
            McpToolRoute::Sentiment => Ok(vec![MCPContent {
                content_type: "text".to_string(),
                text: {
                    let api_key = xai_runtime()?;
                    let tweets_raw = args
                        .get("tweets")
                        .ok_or("Missing tweets array for sentiment analysis")?;
                    let tweets: Vec<Tweet> = serde_json::from_value(tweets_raw.clone())
                        .map_err(|e| format!("Invalid tweets payload for sentiment: {e}"))?;
                    let model = args.get("model").and_then(|v| v.as_str());
                    let http = reqwest::Client::new();
                    let results = sentiment::analyze_sentiment(&http, &api_key, &tweets, model)
                        .await
                        .map_err(|e| format!("Sentiment analysis failed: {e}"))?;
                    let stats = sentiment::compute_stats(&results);
                    serde_json::to_string_pretty(&serde_json::json!({
                        "type": "success",
                        "message": "Sentiment analysis completed.",
                        "data": {
                            "results": results,
                            "stats": {
                                "positive": stats.positive,
                                "negative": stats.negative,
                                "neutral": stats.neutral,
                                "mixed": stats.mixed,
                                "average_score": stats.average_score
                            }
                        }
                    }))
                    .map_err(|e| format!("Failed to encode sentiment payload: {e}"))?
                },
            }]),
            McpToolRoute::Costs => {
                let period = args
                    .get("period")
                    .and_then(|v| v.as_str())
                    .unwrap_or("today");
                let summary = costs::get_cost_summary(&self.costs_path, period);
                let budget = costs::check_budget(&self.costs_path);
                json_content(serde_json::json!({
                    "type": "success",
                    "message": "Cost summary generated.",
                    "data": {
                        "period": period,
                        "summary": summary,
                        "budget": {
                            "allowed": budget.allowed,
                            "spent": budget.spent,
                            "limit": budget.limit,
                            "remaining": budget.remaining,
                            "warning": budget.warning
                        }
                    }
                }))
            }
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use std::sync::{Mutex, OnceLock};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;
    use tokio::sync::oneshot;

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    fn save_env(key: &str) -> Option<String> {
        env::var(key).ok()
    }

    fn restore_env(key: &str, value: Option<String>) {
        if let Some(v) = value {
            env::set_var(key, v);
        } else {
            env::remove_var(key);
        }
    }

    async fn spawn_mock_server(
        status_code: u16,
        response_body: &str,
    ) -> (
        String,
        oneshot::Receiver<String>,
        tokio::task::JoinHandle<()>,
    ) {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind test listener");
        let addr = listener.local_addr().expect("listener local addr");
        let (tx, rx) = oneshot::channel();
        let body = response_body.to_string();

        let handle = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.expect("accept test connection");
            let mut buf = Vec::new();
            let mut chunk = [0_u8; 4096];

            loop {
                let read = socket.read(&mut chunk).await.expect("read request");
                if read == 0 {
                    break;
                }
                buf.extend_from_slice(&chunk[..read]);

                let header_end = buf.windows(4).position(|w| w == b"\r\n\r\n");
                if let Some(end) = header_end {
                    let headers = String::from_utf8_lossy(&buf[..end + 4]).to_string();
                    let content_length = headers
                        .lines()
                        .find_map(|line| {
                            let mut parts = line.splitn(2, ':');
                            let name = parts.next()?.trim().to_lowercase();
                            let value = parts.next()?.trim();
                            if name == "content-length" {
                                return value.parse::<usize>().ok();
                            }
                            None
                        })
                        .unwrap_or(0);
                    let total_needed = end + 4 + content_length;
                    if buf.len() >= total_needed {
                        break;
                    }
                }
            }

            let request_raw = String::from_utf8_lossy(&buf).to_string();
            let _ = tx.send(request_raw);

            let status_text = match status_code {
                202 => "Accepted",
                402 => "Payment Required",
                _ => "OK",
            };
            let response = format!(
                "HTTP/1.1 {status_code} {status_text}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            socket
                .write_all(response.as_bytes())
                .await
                .expect("write response");
        });

        (format!("http://{addr}/v1"), rx, handle)
    }

    #[tokio::test]
    async fn package_create_contract_request_includes_headers_and_payload() {
        let _guard = env_lock().lock().expect("env lock");
        let prev_base = save_env("XINT_PACKAGE_API_BASE_URL");
        let prev_key = save_env("XINT_PACKAGE_API_KEY");
        let prev_workspace = save_env("XINT_WORKSPACE_ID");

        let (base_url, req_rx, server_task) =
            spawn_mock_server(202, r#"{"package_id":"pkg_123","status":"queued"}"#).await;
        env::set_var("XINT_PACKAGE_API_BASE_URL", base_url);
        env::set_var("XINT_PACKAGE_API_KEY", "xck_contract");
        env::set_var("XINT_WORKSPACE_ID", "ws_contract");

        let server = MCPServer::new(
            PolicyMode::ReadOnly,
            false,
            PathBuf::from("/tmp/xint-rs-test-costs.json"),
            PathBuf::from("/tmp/xint-rs-test-reliability.json"),
        );

        let result = server
            .execute_tool(
                "xint_package_create",
                serde_json::json!({
                    "name": "Contract package",
                    "topic_query": "ai agents",
                    "sources": ["x_api_v2"],
                    "time_window": {
                        "from": "2026-01-01T00:00:00.000Z",
                        "to": "2026-01-02T00:00:00.000Z"
                    },
                    "policy": "private",
                    "analysis_profile": "summary"
                }),
            )
            .await
            .expect("package create call");

        let request_raw = req_rx.await.expect("captured request");
        server_task.await.expect("server task");

        let lower = request_raw.to_lowercase();
        assert!(lower.contains("post /v1/packages http/1.1"));
        assert!(lower.contains("authorization: bearer xck_contract"));
        assert!(lower.contains("x-workspace-id: ws_contract"));
        assert!(request_raw.contains("\"name\":\"Contract package\""));
        assert!(request_raw.contains("\"topic_query\":\"ai agents\""));
        assert!(request_raw.contains("\"analysis_profile\":\"summary\""));
        assert!(result[0].text.contains("\"package_id\": \"pkg_123\""));

        restore_env("XINT_PACKAGE_API_BASE_URL", prev_base);
        restore_env("XINT_PACKAGE_API_KEY", prev_key);
        restore_env("XINT_WORKSPACE_ID", prev_workspace);
    }

    #[tokio::test]
    async fn quota_error_includes_upgrade_url() {
        let _guard = env_lock().lock().expect("env lock");
        let prev_base = save_env("XINT_PACKAGE_API_BASE_URL");
        let prev_upgrade = save_env("XINT_BILLING_UPGRADE_URL");

        let (base_url, _req_rx, server_task) = spawn_mock_server(
            402,
            r#"{"code":"QUOTA_EXCEEDED","error":"Package limit reached for current plan."}"#,
        )
        .await;
        env::set_var("XINT_PACKAGE_API_BASE_URL", base_url);
        env::set_var(
            "XINT_BILLING_UPGRADE_URL",
            "https://xint.dev/pricing?src=contract-test",
        );

        let server = MCPServer::new(
            PolicyMode::ReadOnly,
            false,
            PathBuf::from("/tmp/xint-rs-test-costs.json"),
            PathBuf::from("/tmp/xint-rs-test-reliability.json"),
        );

        let err = server
            .call_package_api(
                reqwest::Method::POST,
                "/packages",
                Some(serde_json::json!({})),
            )
            .await
            .expect_err("expected quota error");
        server_task.await.expect("server task");

        assert!(err.contains("QUOTA_EXCEEDED"));
        assert!(err.contains("Upgrade: https://xint.dev/pricing?src=contract-test"));

        restore_env("XINT_PACKAGE_API_BASE_URL", prev_base);
        restore_env("XINT_BILLING_UPGRADE_URL", prev_upgrade);
    }

    #[tokio::test]
    async fn package_query_requires_citations_when_requested() {
        let _guard = env_lock().lock().expect("env lock");
        let prev_base = save_env("XINT_PACKAGE_API_BASE_URL");

        let (base_url, _req_rx, server_task) = spawn_mock_server(
            200,
            r#"{"answer":"No citations","claims":[{"claim_id":"claim_1","text":"example"}],"citations":[]}"#,
        )
        .await;
        env::set_var("XINT_PACKAGE_API_BASE_URL", base_url);

        let server = MCPServer::new(
            PolicyMode::ReadOnly,
            false,
            PathBuf::from("/tmp/xint-rs-test-costs.json"),
            PathBuf::from("/tmp/xint-rs-test-reliability.json"),
        );

        let err = server
            .execute_tool(
                "xint_package_query",
                serde_json::json!({
                    "query": "what changed?",
                    "package_ids": ["pkg_123"],
                    "require_citations": true
                }),
            )
            .await
            .expect_err("expected citation validation failure");
        server_task.await.expect("server task");

        assert!(err.contains("missing citations"));

        restore_env("XINT_PACKAGE_API_BASE_URL", prev_base);
    }

    #[tokio::test]
    async fn costs_tool_returns_success_payload_without_network() {
        let server = MCPServer::new(
            PolicyMode::ReadOnly,
            false,
            PathBuf::from("/tmp/xint-rs-test-costs.json"),
            PathBuf::from("/tmp/xint-rs-test-reliability.json"),
        );

        let result = server
            .execute_tool("xint_costs", serde_json::json!({ "period": "today" }))
            .await
            .expect("costs tool call");

        assert!(result[0].text.contains("\"type\": \"success\""));
        assert!(result[0].text.contains("\"period\": \"today\""));
    }

    #[tokio::test]
    async fn cache_clear_tool_returns_success_payload() {
        let server = MCPServer::new(
            PolicyMode::ReadOnly,
            false,
            PathBuf::from("/tmp/xint-rs-test-costs.json"),
            PathBuf::from("/tmp/xint-rs-test-reliability.json"),
        );

        let result = server
            .execute_tool("xint_cache_clear", serde_json::json!({}))
            .await
            .expect("cache clear tool call");

        assert!(result[0].text.contains("\"type\": \"success\""));
        assert!(result[0].text.contains("\"message\": \"Cache cleared.\""));
        assert!(result[0].text.contains("\"cleared\""));
    }

    #[tokio::test]
    async fn core_search_tool_requires_bearer_token() {
        let _guard = env_lock().lock().expect("env lock");
        let prev_bearer = save_env("X_BEARER_TOKEN");
        env::remove_var("X_BEARER_TOKEN");

        let server = MCPServer::new(
            PolicyMode::ReadOnly,
            false,
            PathBuf::from("/tmp/xint-rs-test-costs.json"),
            PathBuf::from("/tmp/xint-rs-test-reliability.json"),
        );

        let err = server
            .execute_tool("xint_search", serde_json::json!({ "query": "ai agents" }))
            .await
            .expect_err("expected bearer token error");

        assert!(err.contains("X_BEARER_TOKEN"));
        restore_env("X_BEARER_TOKEN", prev_bearer);
    }

    #[tokio::test]
    async fn analyze_tool_requires_xai_api_key() {
        let _guard = env_lock().lock().expect("env lock");
        let prev_key = save_env("XAI_API_KEY");
        env::remove_var("XAI_API_KEY");

        let server = MCPServer::new(
            PolicyMode::ReadOnly,
            false,
            PathBuf::from("/tmp/xint-rs-test-costs.json"),
            PathBuf::from("/tmp/xint-rs-test-reliability.json"),
        );

        let err = server
            .execute_tool(
                "xint_analyze",
                serde_json::json!({ "query": "summarize this" }),
            )
            .await
            .expect_err("expected xai key error");

        assert!(err.contains("XAI_API_KEY"));
        restore_env("XAI_API_KEY", prev_key);
    }
}
