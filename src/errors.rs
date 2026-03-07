//! Structured error types for agent-consumable error responses.

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub enum XintErrorCode {
    #[serde(rename = "RATE_LIMITED")]
    RateLimited,
    #[serde(rename = "AUTH_FAILED")]
    AuthFailed,
    #[serde(rename = "NOT_FOUND")]
    NotFound,
    #[serde(rename = "BUDGET_DENIED")]
    BudgetDenied,
    #[serde(rename = "POLICY_DENIED")]
    PolicyDenied,
    #[serde(rename = "VALIDATION_ERROR")]
    ValidationError,
    #[serde(rename = "TIMEOUT")]
    Timeout,
    #[serde(rename = "API_ERROR")]
    ApiError,
}

#[derive(Debug, Clone, Serialize)]
pub struct XintError {
    pub code: XintErrorCode,
    pub message: String,
    pub retryable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retry_after_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suggestion: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failing_input: Option<String>,
}

impl XintError {
    pub fn rate_limited(wait_ms: u64) -> Self {
        Self {
            code: XintErrorCode::RateLimited,
            message: format!("Rate limited. Retry after {}ms.", wait_ms),
            retryable: true,
            retry_after_ms: Some(wait_ms),
            suggestion: Some(format!("Wait {}ms before retrying.", wait_ms)),
            failing_input: None,
        }
    }

    pub fn auth_failed(detail: &str) -> Self {
        Self {
            code: XintErrorCode::AuthFailed,
            message: detail.to_string(),
            retryable: false,
            retry_after_ms: None,
            suggestion: Some("Set X_BEARER_TOKEN env var or run 'xint auth setup'.".to_string()),
            failing_input: None,
        }
    }

    pub fn not_found(resource: Option<&str>) -> Self {
        Self {
            code: XintErrorCode::NotFound,
            message: resource.map_or("Resource not found.".to_string(), |r| {
                format!("Not found: {}", r)
            }),
            retryable: false,
            retry_after_ms: None,
            suggestion: Some("The tweet or user may have been deleted.".to_string()),
            failing_input: resource.map(|s| s.to_string()),
        }
    }

    pub fn budget_denied(spent: f64, limit: f64) -> Self {
        Self {
            code: XintErrorCode::BudgetDenied,
            message: format!("Daily budget exceeded (${:.2} / ${:.2}).", spent, limit),
            retryable: false,
            retry_after_ms: None,
            suggestion: Some(
                "Use 'xint costs budget set N' to increase the daily limit.".to_string(),
            ),
            failing_input: None,
        }
    }

    pub fn policy_denied(tool: &str, current: &str, required: &str) -> Self {
        Self {
            code: XintErrorCode::PolicyDenied,
            message: format!(
                "Tool '{}' requires '{}' policy mode (current: '{}').",
                tool, required, current
            ),
            retryable: false,
            retry_after_ms: None,
            suggestion: Some(format!("Start MCP with --policy={}.", required)),
            failing_input: Some(tool.to_string()),
        }
    }

    pub fn validation_error(param: &str, reason: &str) -> Self {
        Self {
            code: XintErrorCode::ValidationError,
            message: format!("Parameter '{}': {}", param, reason),
            retryable: false,
            retry_after_ms: None,
            suggestion: Some(format!("Fix parameter '{}': {}", param, reason)),
            failing_input: Some(param.to_string()),
        }
    }

    pub fn timeout(detail: Option<&str>) -> Self {
        Self {
            code: XintErrorCode::Timeout,
            message: detail.unwrap_or("Request timed out.").to_string(),
            retryable: true,
            retry_after_ms: Some(5000),
            suggestion: Some("Retry in 5s.".to_string()),
            failing_input: None,
        }
    }

    pub fn api_error(status: u16, detail: Option<&str>) -> Self {
        let retryable = status >= 500;
        Self {
            code: XintErrorCode::ApiError,
            message: detail
                .unwrap_or(&format!("X API error (HTTP {}).", status))
                .to_string(),
            retryable,
            retry_after_ms: if retryable { Some(30000) } else { None },
            suggestion: if retryable {
                Some("X API issues. Retry in 30s.".to_string())
            } else {
                None
            },
            failing_input: None,
        }
    }

    /// Classify an error string into a structured XintError.
    pub fn classify(msg: &str) -> Self {
        if msg.contains("429") || msg.to_lowercase().contains("rate limit") {
            return Self::rate_limited(60_000);
        }
        if msg.contains("401") || msg.contains("bearer token") || msg.contains("OAuth") {
            return Self::auth_failed(msg);
        }
        if msg.contains("404") || msg.to_lowercase().contains("not found") {
            return Self::not_found(None);
        }
        if msg.contains("BUDGET_DENIED") || msg.to_lowercase().contains("budget exceeded") {
            return Self::budget_denied(0.0, 0.0);
        }
        if msg.contains("POLICY_DENIED") {
            return Self::policy_denied("unknown", "unknown", "unknown");
        }
        if msg.starts_with("Missing ")
            || msg.to_lowercase().contains("invalid ")
            || msg.to_lowercase().contains("required")
        {
            return Self::validation_error("input", msg);
        }
        if msg.to_lowercase().contains("timeout") {
            return Self::timeout(Some(msg));
        }
        Self::api_error(0, Some(msg))
    }
}
