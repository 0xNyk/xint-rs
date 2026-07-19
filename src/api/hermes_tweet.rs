use anyhow::{anyhow, Result};
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION};
use serde_json::Value;
use std::time::Duration;

use crate::models::{Tweet, TweetMetrics};

const DEFAULT_BASE_URL: &str = "https://xquik.com";
const DEFAULT_TIMEOUT_SECONDS: u64 = 30;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TwitterBackend {
    XApiV2,
    HermesTweet,
}

impl TwitterBackend {
    pub fn from_env_value(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "hermes-tweet" | "hermes_tweet" | "xquik" => Self::HermesTweet,
            _ => Self::XApiV2,
        }
    }
}

pub struct HermesTweetClient {
    http: reqwest::Client,
    base_url: String,
    api_key: String,
}

impl HermesTweetClient {
    pub fn new(base_url: Option<&str>, api_key: &str) -> Result<Self> {
        let key = api_key.trim();
        if key.is_empty() {
            return Err(anyhow!(
                "Hermes Tweet backend requires HERMES_TWEET_API_KEY or XQUIK_API_KEY"
            ));
        }
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(DEFAULT_TIMEOUT_SECONDS))
            .build()?;
        Ok(Self {
            http,
            base_url: normalize_base_url(base_url),
            api_key: key.to_string(),
        })
    }

    pub async fn search(&self, query: &str, limit: usize) -> Result<Vec<Tweet>> {
        let url = format!("{}/api/v1/x/tweets/search", self.base_url);
        let limit_param = limit.to_string();
        let response = self
            .http
            .get(url)
            .headers(auth_headers(&self.api_key)?)
            .query(&[("q", query), ("limit", limit_param.as_str())])
            .send()
            .await?;

        let status = response.status();
        let payload: Value = response.json().await?;
        if !status.is_success() {
            return Err(anyhow!(
                "Hermes Tweet backend returned {}: {}",
                status.as_u16(),
                compact_json(&payload)
            ));
        }

        Ok(parse_tweets(&payload))
    }
}

fn normalize_base_url(base_url: Option<&str>) -> String {
    let value = base_url.unwrap_or(DEFAULT_BASE_URL).trim();
    if value.is_empty() {
        return DEFAULT_BASE_URL.to_string();
    }
    value.trim_end_matches('/').to_string()
}

fn auth_headers(api_key: &str) -> Result<HeaderMap> {
    let mut headers = HeaderMap::new();
    let value = api_key.trim();
    if value.to_ascii_lowercase().starts_with("bearer ") {
        headers.insert(AUTHORIZATION, HeaderValue::from_str(value)?);
    } else {
        headers.insert("x-api-key", HeaderValue::from_str(value)?);
    }
    Ok(headers)
}

fn compact_json(value: &Value) -> String {
    serde_json::to_string(value)
        .unwrap_or_default()
        .chars()
        .take(240)
        .collect()
}

pub fn parse_tweets(payload: &Value) -> Vec<Tweet> {
    collect_tweet_candidates(payload)
        .into_iter()
        .filter_map(normalize_tweet)
        .collect()
}

fn collect_tweet_candidates(payload: &Value) -> Vec<&Value> {
    if let Some(items) = payload.as_array() {
        return items.iter().collect();
    }
    let Some(object) = payload.as_object() else {
        return Vec::new();
    };

    for key in ["tweets", "data", "results", "items", "statuses"] {
        if let Some(value) = object.get(key) {
            let nested = collect_tweet_candidates(value);
            if !nested.is_empty() {
                return nested;
            }
        }
    }

    for value in object.values() {
        let nested = collect_tweet_candidates(value);
        if !nested.is_empty() {
            return nested;
        }
    }
    Vec::new()
}

fn normalize_tweet(item: &Value) -> Option<Tweet> {
    let id = first_string(
        item,
        &[
            Path::Key("tweet_id"),
            Path::Key("id"),
            Path::Key("id_str"),
            Path::Key("rest_id"),
            Path::Key("conversation_id"),
        ],
    )?;
    let text = first_string(
        item,
        &[
            Path::Key("source_full_text"),
            Path::Key("full_text"),
            Path::Key("text"),
            Path::Key("content"),
            Path::Key("body"),
        ],
    )
    .unwrap_or_default();
    let username = first_string(
        item,
        &[
            Path::Key("handle"),
            Path::Key("username"),
            Path::Key("screen_name"),
            Path::Nested(&["author", "username"]),
            Path::Nested(&["author", "screen_name"]),
            Path::Nested(&["user", "username"]),
            Path::Nested(&["user", "screen_name"]),
        ],
    )
    .unwrap_or_else(|| "?".to_string())
    .trim_start_matches('@')
    .to_string();
    let name = first_string(
        item,
        &[
            Path::Key("name"),
            Path::Nested(&["author", "name"]),
            Path::Nested(&["user", "name"]),
        ],
    )
    .unwrap_or_else(|| username.clone());
    let author_id = first_string(
        item,
        &[
            Path::Key("author_id"),
            Path::Nested(&["author", "id"]),
            Path::Nested(&["user", "id"]),
        ],
    )
    .unwrap_or_default();
    let created_at = first_string(
        item,
        &[
            Path::Key("created_at"),
            Path::Key("createdAt"),
            Path::Key("timestamp"),
            Path::Key("time"),
        ],
    )
    .unwrap_or_default();
    let conversation_id = first_string(item, &[Path::Key("conversation_id")]).unwrap_or_default();
    let tweet_url = first_string(
        item,
        &[
            Path::Key("tweet_url"),
            Path::Key("status_url"),
            Path::Key("url"),
            Path::Key("link"),
        ],
    )
    .unwrap_or_else(|| {
        if username == "?" {
            String::new()
        } else {
            format!("https://x.com/{username}/status/{id}")
        }
    });

    Some(Tweet {
        id,
        text,
        author_id,
        username,
        name,
        created_at,
        conversation_id,
        metrics: TweetMetrics {
            likes: metric_value(item, &["likes", "like_count"]),
            retweets: metric_value(item, &["retweets", "retweet_count", "reposts"]),
            replies: metric_value(item, &["replies", "reply_count"]),
            quotes: metric_value(item, &["quotes", "quote_count"]),
            impressions: metric_value(item, &["impressions", "impression_count", "views"]),
            bookmarks: metric_value(item, &["bookmarks", "bookmark_count"]),
        },
        urls: Vec::new(),
        mentions: Vec::new(),
        hashtags: Vec::new(),
        tweet_url,
        article: None,
        organic_metrics: None,
        non_public_metrics: None,
    })
}

enum Path<'a> {
    Key(&'a str),
    Nested(&'a [&'a str]),
}

fn first_string(value: &Value, paths: &[Path<'_>]) -> Option<String> {
    for path in paths {
        let current = match path {
            Path::Key(key) => value.get(key),
            Path::Nested(keys) => nested_value(value, keys),
        };
        if let Some(text) = current.and_then(value_to_string) {
            if !text.is_empty() {
                return Some(text);
            }
        }
    }
    None
}

fn nested_value<'a>(value: &'a Value, keys: &[&str]) -> Option<&'a Value> {
    let mut current = value;
    for key in keys {
        current = current.get(*key)?;
    }
    Some(current)
}

fn value_to_string(value: &Value) -> Option<String> {
    if let Some(text) = value.as_str() {
        return Some(text.trim().to_string());
    }
    if let Some(number) = value.as_u64() {
        return Some(number.to_string());
    }
    None
}

fn metric_value(item: &Value, keys: &[&str]) -> u64 {
    for key in keys {
        if let Some(value) = item.get(key).and_then(Value::as_u64) {
            return value;
        }
        if let Some(value) = item
            .get("metrics")
            .and_then(|metrics| metrics.get(*key))
            .and_then(Value::as_u64)
        {
            return value;
        }
        if let Some(value) = item
            .get("public_metrics")
            .and_then(|metrics| metrics.get(*key))
            .and_then(Value::as_u64)
        {
            return value;
        }
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_nested_search_results() {
        let payload = serde_json::json!({
            "data": {
                "tweets": [
                    {
                        "id": "123",
                        "text": "Hermes Agent search result",
                        "author": {"username": "alice", "name": "Alice"},
                        "created_at": "2026-05-23T12:00:00Z",
                        "metrics": {"likes": 9, "retweets": 2}
                    }
                ]
            }
        });

        let tweets = parse_tweets(&payload);

        assert_eq!(tweets.len(), 1);
        assert_eq!(tweets[0].id, "123");
        assert_eq!(tweets[0].username, "alice");
        assert_eq!(tweets[0].tweet_url, "https://x.com/alice/status/123");
        assert_eq!(tweets[0].metrics.likes, 9);
        assert_eq!(tweets[0].metrics.retweets, 2);
    }

    #[test]
    fn accepts_backend_aliases() {
        assert_eq!(
            TwitterBackend::from_env_value("hermes-tweet"),
            TwitterBackend::HermesTweet
        );
        assert_eq!(
            TwitterBackend::from_env_value("xquik"),
            TwitterBackend::HermesTweet
        );
        assert_eq!(
            TwitterBackend::from_env_value("x-api-v2"),
            TwitterBackend::XApiV2
        );
    }
}
