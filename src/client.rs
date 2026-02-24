use anyhow::{bail, Result};
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE};
use std::time::Duration;

use crate::models::RawResponse;

const BASE_URL: &str = "https://api.x.com/2";
const RATE_DELAY_MS: u64 = 350;

pub const FIELDS: &str = "tweet.fields=created_at,public_metrics,author_id,conversation_id,entities,article&expansions=author_id&user.fields=username,name,public_metrics";

/// Shared HTTP client for X API calls.
pub struct XClient {
    http: reqwest::Client,
}

impl XClient {
    pub fn new() -> Result<Self> {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()?;
        Ok(Self { http })
    }

    /// Bearer-authenticated GET request.
    pub async fn bearer_get(&self, path: &str, token: &str) -> Result<RawResponse> {
        let url = if path.starts_with("http") {
            path.to_string()
        } else {
            format!("{BASE_URL}/{path}")
        };

        let res = self
            .http
            .get(&url)
            .header(AUTHORIZATION, format!("Bearer {token}"))
            .send()
            .await?;

        handle_response(res).await
    }

    /// Bearer-authenticated POST request.
    pub async fn bearer_post(
        &self,
        path: &str,
        token: &str,
        body: Option<&serde_json::Value>,
    ) -> Result<serde_json::Value> {
        tokio::time::sleep(Duration::from_millis(RATE_DELAY_MS)).await;

        let url = if path.starts_with("http") {
            path.to_string()
        } else {
            format!("{BASE_URL}/{path}")
        };

        let mut req = self
            .http
            .post(&url)
            .header(AUTHORIZATION, format!("Bearer {token}"));

        if let Some(b) = body {
            req = req.header(CONTENT_TYPE, "application/json").json(b);
        }

        let res = req.send().await?;
        handle_json_response(res, "Bearer token rejected (401). Check X_BEARER_TOKEN.").await
    }

    /// Bearer-authenticated streaming GET request.
    pub async fn bearer_stream(&self, path: &str, token: &str) -> Result<reqwest::Response> {
        let url = if path.starts_with("http") {
            path.to_string()
        } else {
            format!("{BASE_URL}/{path}")
        };

        let res = self
            .http
            .get(&url)
            .header(AUTHORIZATION, format!("Bearer {token}"))
            .send()
            .await?;

        let status = res.status();
        if status.as_u16() == 429 {
            let reset = res
                .headers()
                .get("x-rate-limit-reset")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.parse::<i64>().ok());
            let wait_sec = reset
                .map(|r| {
                    let now = chrono::Utc::now().timestamp();
                    (r - now).max(1)
                })
                .unwrap_or(60);
            bail!("Rate limited. Resets in {wait_sec}s");
        }

        if !status.is_success() {
            let text = res.text().await.unwrap_or_default();
            bail!(
                "X API {}: {}",
                status.as_u16(),
                &text[..text.len().min(200)]
            );
        }

        Ok(res)
    }

    /// OAuth-authenticated GET request (user access token).
    pub async fn oauth_get(&self, path: &str, access_token: &str) -> Result<RawResponse> {
        tokio::time::sleep(Duration::from_millis(RATE_DELAY_MS)).await;

        let url = if path.starts_with("http") {
            path.to_string()
        } else {
            format!("{BASE_URL}/{path}")
        };

        let res = self
            .http
            .get(&url)
            .header(AUTHORIZATION, format!("Bearer {access_token}"))
            .send()
            .await?;

        handle_oauth_response(res).await
    }

    /// OAuth-authenticated POST request.
    pub async fn oauth_post(
        &self,
        path: &str,
        access_token: &str,
        body: Option<&serde_json::Value>,
    ) -> Result<serde_json::Value> {
        tokio::time::sleep(Duration::from_millis(RATE_DELAY_MS)).await;

        let url = if path.starts_with("http") {
            path.to_string()
        } else {
            format!("{BASE_URL}/{path}")
        };

        let mut req = self
            .http
            .post(&url)
            .header(AUTHORIZATION, format!("Bearer {access_token}"));

        if let Some(b) = body {
            req = req.header(CONTENT_TYPE, "application/json").json(b);
        }

        let res = req.send().await?;
        handle_oauth_json(res).await
    }

    /// OAuth-authenticated DELETE request.
    pub async fn oauth_delete(&self, path: &str, access_token: &str) -> Result<serde_json::Value> {
        tokio::time::sleep(Duration::from_millis(RATE_DELAY_MS)).await;

        let url = if path.starts_with("http") {
            path.to_string()
        } else {
            format!("{BASE_URL}/{path}")
        };

        let res = self
            .http
            .delete(&url)
            .header(AUTHORIZATION, format!("Bearer {access_token}"))
            .send()
            .await?;

        handle_oauth_json(res).await
    }

    /// OAuth-authenticated PUT request.
    pub async fn oauth_put(
        &self,
        path: &str,
        access_token: &str,
        body: Option<&serde_json::Value>,
    ) -> Result<serde_json::Value> {
        tokio::time::sleep(Duration::from_millis(RATE_DELAY_MS)).await;

        let url = if path.starts_with("http") {
            path.to_string()
        } else {
            format!("{BASE_URL}/{path}")
        };

        let mut req = self
            .http
            .put(&url)
            .header(AUTHORIZATION, format!("Bearer {access_token}"));

        if let Some(b) = body {
            req = req.header(CONTENT_TYPE, "application/json").json(b);
        }

        let res = req.send().await?;
        handle_oauth_json(res).await
    }

    /// POST with form-encoded body (for token exchange).
    pub async fn post_form(&self, url: &str, params: &[(&str, &str)]) -> Result<serde_json::Value> {
        let res = self
            .http
            .post(url)
            .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
            .form(params)
            .send()
            .await?;

        if !res.status().is_success() {
            let status = res.status().as_u16();
            let text = res.text().await.unwrap_or_default();
            bail!("HTTP {}: {}", status, &text[..text.len().min(200)]);
        }

        Ok(res.json().await?)
    }

    /// Simple GET returning bytes (for webhook etc).
    pub async fn post_json(&self, url: &str, body: &serde_json::Value) -> Result<()> {
        let _ = self
            .http
            .post(url)
            .header(CONTENT_TYPE, "application/json")
            .json(body)
            .send()
            .await;
        Ok(())
    }
}

async fn handle_response(res: reqwest::Response) -> Result<RawResponse> {
    let status = res.status();

    if status.as_u16() == 429 {
        let reset = res
            .headers()
            .get("x-rate-limit-reset")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<i64>().ok());
        let wait_sec = reset
            .map(|r| {
                let now = chrono::Utc::now().timestamp();
                (r - now).max(1)
            })
            .unwrap_or(60);
        bail!("Rate limited. Resets in {wait_sec}s");
    }

    if !status.is_success() {
        let text = res.text().await.unwrap_or_default();
        bail!(
            "X API {}: {}",
            status.as_u16(),
            &text[..text.len().min(200)]
        );
    }

    Ok(res.json().await?)
}

async fn handle_oauth_response(res: reqwest::Response) -> Result<RawResponse> {
    let status = res.status();

    if status.as_u16() == 401 {
        bail!("OAuth token rejected (401). Try 'auth refresh' or re-run 'auth setup'.");
    }

    handle_response(res).await
}

async fn handle_oauth_json(res: reqwest::Response) -> Result<serde_json::Value> {
    handle_json_response(
        res,
        "OAuth token rejected (401). Try 'auth refresh' or re-run 'auth setup'.",
    )
    .await
}

async fn handle_json_response(
    res: reqwest::Response,
    unauthorized_message: &str,
) -> Result<serde_json::Value> {
    let status = res.status();

    if status.as_u16() == 401 {
        bail!("{unauthorized_message}");
    }

    if status.as_u16() == 429 {
        let reset = res
            .headers()
            .get("x-rate-limit-reset")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<i64>().ok());
        let wait_sec = reset
            .map(|r| {
                let now = chrono::Utc::now().timestamp();
                (r - now).max(1)
            })
            .unwrap_or(60);
        bail!("Rate limited. Resets in {wait_sec}s");
    }

    if status.as_u16() == 204 {
        return Ok(serde_json::json!({"success": true}));
    }

    if !status.is_success() {
        let text = res.text().await.unwrap_or_default();
        bail!(
            "X API {}: {}",
            status.as_u16(),
            &text[..text.len().min(200)]
        );
    }

    Ok(res.json().await?)
}

pub async fn rate_delay() {
    tokio::time::sleep(Duration::from_millis(RATE_DELAY_MS)).await;
}
