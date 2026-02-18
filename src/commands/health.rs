use anyhow::Result;
use serde::Serialize;

use crate::auth::oauth;
use crate::cli::HealthArgs;
use crate::client::XClient;
use crate::config::Config;
use crate::costs;
use crate::reliability;

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum StatusLevel {
    Ok,
    Warn,
    Fail,
}

#[derive(Debug, Clone, Serialize)]
struct ServiceCheck {
    configured: bool,
    valid: bool,
    status: StatusLevel,
    message: String,
    latency_ms: Option<u128>,
    status_code: Option<u16>,
}

#[derive(Debug, Clone, Serialize)]
struct OAuthCheck {
    configured: bool,
    valid: bool,
    status: StatusLevel,
    username: Option<String>,
    expires_in_minutes: Option<i64>,
    missing_scopes: Vec<String>,
    message: String,
    status_code: Option<u16>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AuthDoctorReport {
    checked_at: String,
    bearer: ServiceCheck,
    xai: ServiceCheck,
    oauth: OAuthCheck,
    overall_status: StatusLevel,
}

#[derive(Debug, Clone, Serialize)]
struct BudgetView {
    allowed: bool,
    warning: bool,
    spent_usd: f64,
    limit_usd: f64,
    remaining_usd: f64,
}

#[derive(Debug, Clone, Serialize)]
struct TodayView {
    calls: u64,
    tweets_read: u64,
    total_cost_usd: f64,
}

#[derive(Debug, Clone, Serialize)]
struct HealthReport {
    checked_at: String,
    overall_status: StatusLevel,
    auth: AuthDoctorReport,
    budget: BudgetView,
    today: TodayView,
    reliability: reliability::ReliabilityReport,
}

const REQUIRED_OAUTH_SCOPES: &[&str] = &[
    "bookmark.read",
    "bookmark.write",
    "like.read",
    "like.write",
    "follows.read",
    "follows.write",
    "list.read",
    "list.write",
    "block.read",
    "block.write",
    "mute.read",
    "mute.write",
    "tweet.read",
    "tweet.write",
    "users.read",
    "offline.access",
];

fn combine_status(current: StatusLevel, next: StatusLevel) -> StatusLevel {
    let rank = |s: StatusLevel| match s {
        StatusLevel::Ok => 1,
        StatusLevel::Warn => 2,
        StatusLevel::Fail => 3,
    };
    if rank(next) > rank(current) {
        next
    } else {
        current
    }
}

fn icon(status: StatusLevel) -> &'static str {
    match status {
        StatusLevel::Ok => "✅",
        StatusLevel::Warn => "⚠️",
        StatusLevel::Fail => "❌",
    }
}

async fn check_bearer(config: &Config, client: &XClient) -> ServiceCheck {
    let token = match config.bearer_token.as_deref() {
        Some(token) => token,
        None => {
            return ServiceCheck {
                configured: false,
                valid: false,
                status: StatusLevel::Fail,
                message: "X_BEARER_TOKEN missing".to_string(),
                latency_ms: None,
                status_code: None,
            }
        }
    };

    let started = std::time::Instant::now();
    match client
        .bearer_get("users/by/username/x?user.fields=id", token)
        .await
    {
        Ok(_) => ServiceCheck {
            configured: true,
            valid: true,
            status: StatusLevel::Ok,
            message: "Bearer token valid".to_string(),
            latency_ms: Some(started.elapsed().as_millis()),
            status_code: Some(200),
        },
        Err(err) => ServiceCheck {
            configured: true,
            valid: false,
            status: StatusLevel::Fail,
            message: format!("Bearer token check failed: {err}"),
            latency_ms: Some(started.elapsed().as_millis()),
            status_code: None,
        },
    }
}

async fn check_xai(config: &Config) -> ServiceCheck {
    let key = match config.xai_api_key.as_deref() {
        Some(key) => key,
        None => {
            return ServiceCheck {
                configured: false,
                valid: false,
                status: StatusLevel::Warn,
                message: "XAI_API_KEY missing (AI features disabled)".to_string(),
                latency_ms: None,
                status_code: None,
            }
        }
    };

    let started = std::time::Instant::now();
    let http = reqwest::Client::new();
    match http
        .get("https://api.x.ai/v1/models")
        .bearer_auth(key)
        .send()
        .await
    {
        Ok(res) => {
            let status = res.status().as_u16();
            if res.status().is_success() {
                ServiceCheck {
                    configured: true,
                    valid: true,
                    status: StatusLevel::Ok,
                    message: "xAI key valid".to_string(),
                    latency_ms: Some(started.elapsed().as_millis()),
                    status_code: Some(status),
                }
            } else {
                ServiceCheck {
                    configured: true,
                    valid: false,
                    status: StatusLevel::Fail,
                    message: format!("xAI key check failed ({status})"),
                    latency_ms: Some(started.elapsed().as_millis()),
                    status_code: Some(status),
                }
            }
        }
        Err(err) => ServiceCheck {
            configured: true,
            valid: false,
            status: StatusLevel::Warn,
            message: format!("xAI key check error: {err}"),
            latency_ms: Some(started.elapsed().as_millis()),
            status_code: None,
        },
    }
}

async fn check_oauth(config: &Config, client: &XClient) -> OAuthCheck {
    let tokens = match oauth::load_tokens(&config.tokens_path()) {
        Some(tokens) => tokens,
        None => {
            return OAuthCheck {
                configured: false,
                valid: false,
                status: StatusLevel::Warn,
                username: None,
                expires_in_minutes: None,
                missing_scopes: REQUIRED_OAUTH_SCOPES
                    .iter()
                    .map(|scope| scope.to_string())
                    .collect(),
                message: "OAuth tokens missing (engagement/moderation commands unavailable)"
                    .to_string(),
                status_code: None,
            }
        }
    };

    let now_ms = chrono::Utc::now().timestamp_millis();
    let expires_in_minutes = (tokens.expires_at - now_ms) / 60_000;

    let scope_set: std::collections::HashSet<_> = tokens.scope.split_whitespace().collect();
    let missing_scopes: Vec<String> = REQUIRED_OAUTH_SCOPES
        .iter()
        .filter(|scope| !scope_set.contains(**scope))
        .map(|scope| scope.to_string())
        .collect();

    let (mut status, valid, mut message, status_code) =
        match client.oauth_get("users/me", &tokens.access_token).await {
            Ok(_) => (
                StatusLevel::Ok,
                true,
                "OAuth access token valid".to_string(),
                Some(200),
            ),
            Err(err) => (
                StatusLevel::Fail,
                false,
                format!("OAuth token check failed: {err}"),
                None,
            ),
        };

    if expires_in_minutes <= 0 {
        status = combine_status(status, StatusLevel::Warn);
        message = "OAuth access token expired (refresh required)".to_string();
    }

    if !missing_scopes.is_empty() {
        status = combine_status(status, StatusLevel::Warn);
        message = format!("{message}; missing scopes: {}", missing_scopes.join(", "));
    }

    OAuthCheck {
        configured: true,
        valid,
        status,
        username: Some(tokens.username),
        expires_in_minutes: Some(expires_in_minutes),
        missing_scopes,
        message,
        status_code,
    }
}

fn status_label(status: StatusLevel) -> &'static str {
    match status {
        StatusLevel::Ok => "ok",
        StatusLevel::Warn => "warn",
        StatusLevel::Fail => "fail",
    }
}

pub async fn auth_doctor_report(config: &Config, client: &XClient) -> AuthDoctorReport {
    let bearer = check_bearer(config, client).await;
    let xai = check_xai(config).await;
    let oauth = check_oauth(config, client).await;

    let mut overall = StatusLevel::Ok;
    overall = combine_status(overall, bearer.status);
    overall = combine_status(overall, xai.status);
    overall = combine_status(overall, oauth.status);

    AuthDoctorReport {
        checked_at: chrono::Utc::now().to_rfc3339(),
        bearer,
        xai,
        oauth,
        overall_status: overall,
    }
}

pub async fn run_auth_doctor(
    config: &Config,
    client: &XClient,
    as_json: bool,
) -> Result<AuthDoctorReport> {
    let report = auth_doctor_report(config, client).await;

    if as_json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!(
            "{} Auth Doctor ({})",
            icon(report.overall_status),
            report.checked_at
        );
        println!(
            "- Bearer: {} {}",
            icon(report.bearer.status),
            report.bearer.message
        );
        println!("- xAI: {} {}", icon(report.xai.status), report.xai.message);
        let oauth_user = report
            .oauth
            .username
            .as_ref()
            .map(|name| format!("@{name}"))
            .unwrap_or_else(|| "not configured".to_string());
        println!(
            "- OAuth: {} {} ({oauth_user})",
            icon(report.oauth.status),
            report.oauth.message
        );
    }

    Ok(report)
}

pub async fn run(args: &HealthArgs, config: &Config, client: &XClient) -> Result<()> {
    let auth = auth_doctor_report(config, client).await;
    let budget = costs::check_budget(&config.costs_path());
    let today = costs::today_costs(&config.costs_path());
    let reliability = reliability::get_reliability_report(&config.reliability_path(), args.days);

    let mut overall = auth.overall_status;
    if !budget.allowed {
        overall = combine_status(overall, StatusLevel::Warn);
    }
    if reliability.total_calls > 0 && reliability.success_rate < 0.9 {
        overall = combine_status(overall, StatusLevel::Warn);
    }

    let report = HealthReport {
        checked_at: chrono::Utc::now().to_rfc3339(),
        overall_status: overall,
        auth,
        budget: BudgetView {
            allowed: budget.allowed,
            warning: budget.warning,
            spent_usd: budget.spent,
            limit_usd: budget.limit,
            remaining_usd: budget.remaining,
        },
        today: TodayView {
            calls: today.calls,
            tweets_read: today.tweets_read,
            total_cost_usd: today.total_cost,
        },
        reliability,
    };

    if args.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }

    println!(
        "{} xint Health ({})",
        icon(report.overall_status),
        report.checked_at
    );
    println!(
        "- Auth status: {} {}",
        icon(report.auth.overall_status),
        status_label(report.auth.overall_status)
    );
    println!(
        "- Budget: {} (${:.2} / ${:.2})",
        if report.budget.allowed {
            "within limit"
        } else {
            "exceeded"
        },
        report.budget.spent_usd,
        report.budget.limit_usd
    );
    println!(
        "- Reliability ({}d): {:.1}% success across {} calls",
        report.reliability.window_days,
        report.reliability.success_rate * 100.0,
        report.reliability.total_calls
    );

    let mut top_commands: Vec<_> = report.reliability.by_command.iter().collect();
    top_commands.sort_by(|a, b| b.1.calls.cmp(&a.1.calls));

    if !top_commands.is_empty() {
        println!("- Top commands:");
        for (command, stats) in top_commands.into_iter().take(5) {
            println!(
                "  {}: {:.1}% ok, p95 {:.0}ms, fallback {:.1}%",
                command,
                stats.success_rate * 100.0,
                stats.p95_latency_ms,
                stats.fallback_rate * 100.0
            );
        }
    }

    Ok(())
}
