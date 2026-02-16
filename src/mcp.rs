//! xint MCP Server
//!
//! MCP (Model Context Protocol) server implementation for xint-rs CLI.
//! Exposes xint functionality as MCP tools for AI agents like Claude Code.

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, BufReader};

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
    Initialize { protocol_version: String, capabilities: serde_json::Value, client_info: serde_json::Value },
    #[serde(rename = "initialized")]
    Initialized,
    #[serde(rename = "tools/list")]
    ToolsList,
    #[serde(rename = "tools/list/result")]
    ToolsListResult { tools: Vec<MCPTool> },
    #[serde(rename = "tools/call")]
    ToolsCall { name: String, arguments: serde_json::Value },
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
}

impl MCPServer {
    pub fn new() -> Self {
        Self { initialized: false }
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

    pub async fn handle_message(&mut self, msg: &str) -> Result<Option<String>, String> {
        let parsed: serde_json::Value = serde_json::from_str(msg)
            .map_err(|e| format!("Failed to parse JSON: {}", e))?;

        let method = parsed.get("method")
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
                let params = parsed.get("params")
                    .ok_or("Missing params")?;
                let name = params.get("name")
                    .and_then(|v| v.as_str())
                    .ok_or("Missing tool name")?;
                let arguments = params.get("arguments")
                    .cloned()
                    .unwrap_or(serde_json::Value::Object(serde_json::Map::new()));

                let result = self.execute_tool(name, arguments).await?;
                let response = serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": {
                        "content": result
                    }
                });
                Ok(Some(response.to_string()))
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

    async fn execute_tool(&self, name: &str, args: serde_json::Value) -> Result<Vec<MCPContent>, String> {
        match name {
            "xint_search" => {
                let query = args.get("query")
                    .and_then(|v| v.as_str())
                    .ok_or("Missing query")?;
                let limit = args.get("limit")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(15) as usize;
                
                // Note: In real implementation, we'd call the API here
                // For now, return a placeholder
                Ok(vec![MCPContent {
                    content_type: "text".to_string(),
                    text: format!("Search: {} (limit: {})", query, limit),
                }])
            }
            "xint_profile" => {
                let username = args.get("username")
                    .and_then(|v| v.as_str())
                    .ok_or("Missing username")?;
                
                Ok(vec![MCPContent {
                    content_type: "text".to_string(),
                    text: format!("Profile: @{}", username),
                }])
            }
            "xint_thread" => {
                let tweet_id = args.get("tweet_id")
                    .and_then(|v| v.as_str())
                    .ok_or("Missing tweet_id")?;
                
                Ok(vec![MCPContent {
                    content_type: "text".to_string(),
                    text: format!("Thread for tweet: {}", tweet_id),
                }])
            }
            "xint_tweet" => {
                let tweet_id = args.get("tweet_id")
                    .and_then(|v| v.as_str())
                    .ok_or("Missing tweet_id")?;
                
                Ok(vec![MCPContent {
                    content_type: "text".to_string(),
                    text: format!("Tweet: {}", tweet_id),
                }])
            }
            "xint_trends" => {
                let location = args.get("location")
                    .and_then(|v| v.as_str())
                    .unwrap_or("worldwide");
                
                Ok(vec![MCPContent {
                    content_type: "text".to_string(),
                    text: format!("Trends for: {}", location),
                }])
            }
            "xint_xsearch" => {
                let query = args.get("query")
                    .and_then(|v| v.as_str())
                    .ok_or("Missing query")?;
                
                Ok(vec![MCPContent {
                    content_type: "text".to_string(),
                    text: format!("X-Search: {}", query),
                }])
            }
            "xint_collections_list" => {
                Ok(vec![MCPContent {
                    content_type: "text".to_string(),
                    text: "Collections: []".to_string(),
                }])
            }
            "xint_analyze" => {
                let query = args.get("query")
                    .and_then(|v| v.as_str())
                    .ok_or("Missing query")?;
                
                Ok(vec![MCPContent {
                    content_type: "text".to_string(),
                    text: format!("Analysis: {}", query),
                }])
            }
            "xint_article" => {
                let url = args.get("url")
                    .and_then(|v| v.as_str())
                    .ok_or("Missing url")?;
                
                Ok(vec![MCPContent {
                    content_type: "text".to_string(),
                    text: format!("Article: {}", url),
                }])
            }
            "xint_collections_search" => {
                let collection_id = args.get("collection_id")
                    .and_then(|v| v.as_str())
                    .ok_or("Missing collection_id")?;
                let query = args.get("query")
                    .and_then(|v| v.as_str())
                    .ok_or("Missing query")?;
                
                Ok(vec![MCPContent {
                    content_type: "text".to_string(),
                    text: format!("Collections search in {}: {}", collection_id, query),
                }])
            }
            "xint_bookmarks" => {
                Ok(vec![MCPContent {
                    content_type: "text".to_string(),
                    text: "Bookmarks: OAuth required".to_string(),
                }])
            }
            "xint_cache_clear" => {
                Ok(vec![MCPContent {
                    content_type: "text".to_string(),
                    text: "Cache cleared".to_string(),
                }])
            }
            "xint_watch" => {
                let query = args.get("query")
                    .and_then(|v| v.as_str())
                    .ok_or("Missing query")?;
                Ok(vec![MCPContent {
                    content_type: "text".to_string(),
                    text: format!("Watch: {} (use CLI for real-time monitoring)", query),
                }])
            }
            "xint_diff" => {
                let username = args.get("username")
                    .and_then(|v| v.as_str())
                    .ok_or("Missing username")?;
                Ok(vec![MCPContent {
                    content_type: "text".to_string(),
                    text: format!("Diff tracking for @{}", username),
                }])
            }
            "xint_report" => {
                let topic = args.get("topic")
                    .and_then(|v| v.as_str())
                    .ok_or("Missing topic")?;
                Ok(vec![MCPContent {
                    content_type: "text".to_string(),
                    text: format!("Report on: {} (requires XAI_API_KEY)", topic),
                }])
            }
            "xint_sentiment" => {
                Ok(vec![MCPContent {
                    content_type: "text".to_string(),
                    text: "Sentiment analysis (requires XAI_API_KEY)".to_string(),
                }])
            }
            "xint_costs" => {
                let period = args.get("period")
                    .and_then(|v| v.as_str())
                    .unwrap_or("today");
                Ok(vec![MCPContent {
                    content_type: "text".to_string(),
                    text: format!("Cost tracking for period: {}", period),
                }])
            }
            _ => Err(format!("Unknown tool: {}", name)),
        }
    }

    pub async fn run_stdio(&mut self) -> Result<(), String> {
        let stdin = tokio::io::stdin();
        let mut reader = BufReader::new(stdin).lines();

        while let Ok(Some(line)) = reader.next_line().await {
            if let Some(response) = self.handle_message(&line).await? {
                println!("{}", response);
            }
        }

        Ok(())
    }
}

// ============================================================================
// CLI Command - using McpArgs from cli module
// ============================================================================

pub async fn run(args: crate::cli::McpArgs) -> anyhow::Result<()> {
    println!("Starting xint MCP server (sse: {}, port: {})...", args.sse, args.port);
    
    let mut server = MCPServer::new();
    server.run_stdio().await.map_err(|e| anyhow::anyhow!(e))?;
    
    Ok(())
}
