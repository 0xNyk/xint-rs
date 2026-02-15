use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};

const API_BASE: &str = "https://api.x.ai/v1";
const MGMT_BASE: &str = "https://management-api.x.ai/v1";

// ---------------------------------------------------------------------------
// Responses API — x_search tool
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct XSearchResult {
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub tweet_url: Option<String>,
    #[serde(default)]
    pub link: Option<String>,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub content: Option<String>,
    #[serde(default)]
    pub snippet: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub username: Option<String>,
    #[serde(default)]
    pub author: Option<String>,
    #[serde(default)]
    pub handle: Option<String>,
    #[serde(default)]
    pub created_at: Option<String>,
    #[serde(default)]
    pub date: Option<String>,
    #[serde(default)]
    pub timestamp: Option<String>,
}

impl XSearchResult {
    pub fn best_url(&self) -> String {
        self.url
            .as_deref()
            .or(self.tweet_url.as_deref())
            .or(self.link.as_deref())
            .unwrap_or("")
            .to_string()
    }

    pub fn best_text(&self) -> String {
        self.text
            .as_deref()
            .or(self.content.as_deref())
            .or(self.snippet.as_deref())
            .or(self.title.as_deref())
            .map(|s| s.trim())
            .unwrap_or("")
            .to_string()
    }

    pub fn best_handle(&self) -> String {
        self.username
            .as_deref()
            .or(self.author.as_deref())
            .or(self.handle.as_deref())
            .map(|s| s.trim().trim_start_matches('@'))
            .unwrap_or("")
            .to_string()
    }

    pub fn best_created_at(&self) -> String {
        self.created_at
            .as_deref()
            .or(self.date.as_deref())
            .or(self.timestamp.as_deref())
            .map(|s| s.trim())
            .unwrap_or("")
            .to_string()
    }
}

/// Call xAI Responses API with x_search tool to search X.
pub async fn x_search(
    http: &reqwest::Client,
    api_key: &str,
    query: &str,
    max_results: u32,
    from_date: Option<&str>,
    to_date: Option<&str>,
    model: &str,
    timeout_secs: u64,
) -> Result<(Vec<XSearchResult>, String)> {
    let mut tool_spec = serde_json::json!({
        "type": "x_search",
        "max_results": max_results,
    });
    if let Some(fd) = from_date {
        if !fd.is_empty() {
            tool_spec["from_date"] = serde_json::Value::String(fd.to_string());
        }
    }
    if let Some(td) = to_date {
        if !td.is_empty() {
            tool_spec["to_date"] = serde_json::Value::String(td.to_string());
        }
    }

    let body = serde_json::json!({
        "model": model,
        "input": format!("Use x_search to find recent posts relevant to: {}\nReturn the search results.", query),
        "tools": [tool_spec],
        "tool_choice": "required",
        "max_output_tokens": 800,
    });

    let url = format!("{}/responses", API_BASE);

    let res = http
        .post(&url)
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
        .timeout(std::time::Duration::from_secs(timeout_secs))
        .json(&body)
        .send()
        .await?;

    let status = res.status().as_u16();
    if status == 401 {
        bail!("xAI auth failed (401). Check your XAI_API_KEY.");
    }
    if status == 402 {
        bail!("xAI payment required (402). Your account may be out of credits.");
    }
    if status == 429 {
        bail!("xAI rate limited (429). Try again in a moment.");
    }
    if !res.status().is_success() {
        let text = res.text().await.unwrap_or_default();
        bail!("xAI API error ({}): {}", status, &text[..text.len().min(500)]);
    }

    let data: serde_json::Value = res.json().await?;

    // Extract x_search results + output text from Responses API format
    let mut results = Vec::new();
    let mut output_text = String::new();

    if let Some(output) = data.get("output").and_then(|v| v.as_array()) {
        for item in output {
            let item_type = item.get("type").and_then(|v| v.as_str()).unwrap_or("");

            if item_type == "x_search_call" {
                if let Some(r) = item.get("results").and_then(|v| v.as_array()) {
                    for rr in r {
                        if let Ok(sr) = serde_json::from_value::<XSearchResult>(rr.clone()) {
                            results.push(sr);
                        }
                    }
                }
            }

            if item_type == "message" {
                if let Some(content) = item.get("content").and_then(|v| v.as_array()) {
                    let mut parts = Vec::new();
                    for part in content {
                        if part.get("type").and_then(|v| v.as_str()) == Some("output_text") {
                            if let Some(txt) = part.get("text").and_then(|v| v.as_str()) {
                                let trimmed = txt.trim();
                                if !trimmed.is_empty() {
                                    parts.push(trimmed.to_string());
                                }
                            }
                        }
                    }
                    if !parts.is_empty() {
                        output_text = parts.join("\n");
                    }
                }
            }
        }
    }

    Ok((results, output_text))
}

// ---------------------------------------------------------------------------
// Responses API — web_search tool (article fetching)
// ---------------------------------------------------------------------------

/// Call xAI Responses API with web_search tool to fetch article content from a URL.
pub async fn web_search_article(
    http: &reqwest::Client,
    api_key: &str,
    url: &str,
    domain: &str,
    model: &str,
    timeout_secs: u64,
) -> Result<String> {
    let prompt = format!(
        "Read the article at this URL and extract its content. Return a JSON object with these fields:\n\
         - title: article title\n\
         - description: 1-2 sentence summary\n\
         - content: the full article text (plain text, no HTML)\n\
         - author: author name (empty string if unknown)\n\
         - published: publication date (empty string if unknown)\n\n\
         Return ONLY valid JSON, no markdown fences, no explanation.\n\nURL: {}",
        url
    );

    let body = serde_json::json!({
        "model": model,
        "tools": [{
            "type": "web_search",
            "allowed_domains": [domain],
        }],
        "input": [{
            "role": "user",
            "content": prompt,
        }],
    });

    let api_url = format!("{}/responses", API_BASE);

    let res = http
        .post(&api_url)
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
        .timeout(std::time::Duration::from_secs(timeout_secs))
        .json(&body)
        .send()
        .await?;

    let status = res.status().as_u16();
    if !res.status().is_success() {
        let text = res.text().await.unwrap_or_default();
        bail!("xAI API error ({}): {}", status, &text[..text.len().min(500)]);
    }

    let data: serde_json::Value = res.json().await?;

    // Extract output text from Responses API format
    if let Some(output) = data.get("output").and_then(|v| v.as_array()) {
        for item in output {
            let item_type = item.get("type").and_then(|v| v.as_str()).unwrap_or("");
            if item_type == "message" {
                if let Some(content) = item.get("content").and_then(|v| v.as_array()) {
                    for part in content {
                        let part_type = part.get("type").and_then(|v| v.as_str()).unwrap_or("");
                        if part_type == "output_text" || part_type == "text" {
                            if let Some(txt) = part.get("text").and_then(|v| v.as_str()) {
                                let trimmed = txt.trim();
                                if !trimmed.is_empty() {
                                    return Ok(trimmed.to_string());
                                }
                            }
                        }
                    }
                }
            }
            // Direct text field
            if let Some(txt) = item.get("text").and_then(|v| v.as_str()) {
                let trimmed = txt.trim();
                if !trimmed.is_empty() {
                    return Ok(trimmed.to_string());
                }
            }
        }
    }

    bail!("No content returned for {}", url)
}

// ---------------------------------------------------------------------------
// Management API — Collections
// ---------------------------------------------------------------------------

/// List all collections (Management API).
pub async fn collections_list(
    http: &reqwest::Client,
    mgmt_key: &str,
) -> Result<serde_json::Value> {
    let url = format!("{}/collections", MGMT_BASE);
    let res = http
        .get(&url)
        .header("Authorization", format!("Bearer {}", mgmt_key))
        .send()
        .await?;

    handle_xai_response(res, "GET /collections").await
}

/// Create a collection (Management API).
pub async fn collections_create(
    http: &reqwest::Client,
    mgmt_key: &str,
    name: &str,
    description: &str,
) -> Result<serde_json::Value> {
    let url = format!("{}/collections", MGMT_BASE);
    let mut body = serde_json::json!({"name": name});
    if !description.is_empty() {
        body["description"] = serde_json::Value::String(description.to_string());
    }

    let res = http
        .post(&url)
        .header("Authorization", format!("Bearer {}", mgmt_key))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await?;

    handle_xai_response(res, "POST /collections").await
}

/// Ensure a collection exists (list → create if missing).
pub async fn collections_ensure(
    http: &reqwest::Client,
    mgmt_key: &str,
    name: &str,
    description: &str,
) -> Result<serde_json::Value> {
    let list = collections_list(http, mgmt_key).await?;

    if let Some(items) = list.get("data").and_then(|v| v.as_array()) {
        for item in items {
            if item.get("name").and_then(|v| v.as_str()) == Some(name) {
                return Ok(serde_json::json!({"status": "ok", "collection": item}));
            }
        }
    }

    let created = collections_create(http, mgmt_key, name, description).await?;
    Ok(serde_json::json!({"status": "ok", "created": created}))
}

/// Add a document to a collection (Management API).
pub async fn collections_add_document(
    http: &reqwest::Client,
    mgmt_key: &str,
    collection_id: &str,
    document_id: &str,
) -> Result<serde_json::Value> {
    let url = format!("{}/collections/{}/documents", MGMT_BASE, collection_id);
    let body = serde_json::json!({"document_id": document_id});

    let res = http
        .post(&url)
        .header("Authorization", format!("Bearer {}", mgmt_key))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await?;

    handle_xai_response(res, &format!("POST /collections/{}/documents", collection_id)).await
}

/// Upload a file to xAI (Files API).
pub async fn files_upload(
    http: &reqwest::Client,
    api_key: &str,
    file_path: &std::path::Path,
    purpose: &str,
) -> Result<serde_json::Value> {
    let url = format!("{}/files", API_BASE);

    let file_name = file_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("file")
        .to_string();

    let file_bytes = std::fs::read(file_path)?;
    let mime = mime_from_path(file_path);

    let form = reqwest::multipart::Form::new()
        .part(
            "file",
            reqwest::multipart::Part::bytes(file_bytes)
                .file_name(file_name)
                .mime_str(&mime)?,
        )
        .text("purpose", purpose.to_string());

    let res = http
        .post(&url)
        .header("Authorization", format!("Bearer {}", api_key))
        .multipart(form)
        .timeout(std::time::Duration::from_secs(90))
        .send()
        .await?;

    handle_xai_response(res, "POST /files").await
}

/// Search documents (API).
pub async fn documents_search(
    http: &reqwest::Client,
    api_key: &str,
    collection_ids: &[String],
    query: &str,
    top_k: u32,
) -> Result<serde_json::Value> {
    let url = format!("{}/documents/search", API_BASE);
    let mut body = serde_json::json!({
        "query": query,
        "top_k": top_k,
    });
    if !collection_ids.is_empty() {
        body["collection_ids"] = serde_json::Value::Array(
            collection_ids
                .iter()
                .map(|s| serde_json::Value::String(s.clone()))
                .collect(),
        );
    }

    let res = http
        .post(&url)
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await?;

    handle_xai_response(res, "POST /documents/search").await
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

async fn handle_xai_response(res: reqwest::Response, context: &str) -> Result<serde_json::Value> {
    let status = res.status().as_u16();
    if status == 401 {
        bail!("xAI auth failed (401) on {}. Check your API key.", context);
    }
    if status == 402 {
        bail!("xAI payment required (402). Your account may be out of credits.");
    }
    if status == 429 {
        bail!("xAI rate limited (429). Try again in a moment.");
    }
    if !res.status().is_success() {
        let text = res.text().await.unwrap_or_default();
        bail!(
            "xAI API error ({}) on {}: {}",
            status,
            context,
            &text[..text.len().min(500)]
        );
    }

    let text = res.text().await.unwrap_or_default();
    if text.is_empty() {
        return Ok(serde_json::json!({}));
    }
    Ok(serde_json::from_str(&text)?)
}

fn mime_from_path(path: &std::path::Path) -> String {
    match path.extension().and_then(|e| e.to_str()) {
        Some("md") => "text/markdown",
        Some("txt") => "text/plain",
        Some("json") => "application/json",
        Some("jsonl") => "application/x-ndjson",
        Some("csv") => "text/csv",
        Some("pdf") => "application/pdf",
        Some("html") | Some("htm") => "text/html",
        _ => "application/octet-stream",
    }
    .to_string()
}
