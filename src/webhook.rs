use anyhow::{bail, Result};
use url::Url;

const WEBHOOK_ALLOWLIST_ENV: &str = "XINT_WEBHOOK_ALLOWED_HOSTS";

fn is_loopback_host(hostname: &str) -> bool {
    matches!(hostname, "localhost" | "127.0.0.1" | "::1")
}

fn parse_allowlist_from(raw: Option<&str>) -> Vec<String> {
    raw.map(|value| {
        value
            .split(',')
            .map(|item| item.trim().to_lowercase())
            .filter(|item| !item.is_empty())
            .collect::<Vec<_>>()
    })
    .unwrap_or_default()
}

fn host_allowed_by_rule(hostname: &str, rule: &str) -> bool {
    if let Some(suffix) = rule.strip_prefix("*.") {
        hostname == suffix || hostname.ends_with(&format!(".{suffix}"))
    } else {
        hostname == rule
    }
}

pub fn validate_webhook_url(raw_url: &str) -> Result<String> {
    let allowlist_raw = std::env::var(WEBHOOK_ALLOWLIST_ENV).ok();
    validate_webhook_url_with_allowlist(raw_url, allowlist_raw.as_deref())
}

fn validate_webhook_url_with_allowlist(
    raw_url: &str,
    allowlist_raw: Option<&str>,
) -> Result<String> {
    let url = Url::parse(raw_url).map_err(|_| anyhow::anyhow!("Invalid webhook URL."))?;
    if !url.username().is_empty() || url.password().is_some() {
        bail!("Webhook URL must not include credentials.");
    }

    let hostname = url
        .host_str()
        .ok_or_else(|| anyhow::anyhow!("Webhook URL must include a host."))?
        .to_lowercase();
    let scheme = url.scheme().to_lowercase();
    let is_loopback = is_loopback_host(&hostname);

    if scheme != "https" && !(is_loopback && scheme == "http") {
        bail!(
            "Webhook URL must use https:// (http:// is only allowed for localhost/127.0.0.1/::1)."
        );
    }

    let allowlist = parse_allowlist_from(allowlist_raw);
    if !allowlist.is_empty()
        && !allowlist
            .iter()
            .any(|rule| host_allowed_by_rule(&hostname, rule))
    {
        bail!(
            "Webhook host '{hostname}' is not allowed. Set {WEBHOOK_ALLOWLIST_ENV} to include it."
        );
    }

    Ok(url.to_string())
}

#[cfg(test)]
mod tests {
    use super::validate_webhook_url_with_allowlist;

    #[test]
    fn accepts_https_webhook() {
        let url =
            validate_webhook_url_with_allowlist("https://hooks.example.com/ingest", None).unwrap();
        assert_eq!(url, "https://hooks.example.com/ingest");
    }

    #[test]
    fn allows_http_for_loopback() {
        let url =
            validate_webhook_url_with_allowlist("http://127.0.0.1:8080/webhook", None).unwrap();
        assert_eq!(url, "http://127.0.0.1:8080/webhook");
    }

    #[test]
    fn rejects_http_for_remote_hosts() {
        let err = validate_webhook_url_with_allowlist("http://example.com/webhook", None)
            .expect_err("http remote webhooks must be rejected");
        assert!(err.to_string().contains("Webhook URL must use https://"));
    }

    #[test]
    fn enforces_allowlist_when_set() {
        let allowlist = Some("trusted.example.com,*.internal.example");
        let trusted =
            validate_webhook_url_with_allowlist("https://trusted.example.com/path", allowlist)
                .unwrap();
        assert_eq!(trusted, "https://trusted.example.com/path");

        let wildcard =
            validate_webhook_url_with_allowlist("https://api.internal.example/hook", allowlist)
                .unwrap();
        assert_eq!(wildcard, "https://api.internal.example/hook");

        let err =
            validate_webhook_url_with_allowlist("https://untrusted.example.com/hook", allowlist)
                .expect_err("untrusted host must be rejected");
        assert!(err.to_string().contains("is not allowed"));
    }
}
