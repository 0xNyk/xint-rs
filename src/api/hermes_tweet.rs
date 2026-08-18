use anyhow::{anyhow, Result};
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, RETRY_AFTER};
use serde::Deserialize;
use serde_json::Value;
use std::time::Duration;

use crate::models::{Tweet, TweetMetrics};

const DEFAULT_BASE_URL: &str = "https://xquik.com";
const DEFAULT_TIMEOUT_SECONDS: u64 = 30;
const MAX_ATTEMPTS: u32 = 3;

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
            base_url: normalize_base_url(base_url)?,
            api_key: key.to_string(),
        })
    }

    pub async fn search(&self, query: &str, limit: usize) -> Result<Vec<Tweet>> {
        let url = format!("{}/api/v1/x/tweets/search", self.base_url);
        let limit_param = limit.to_string();
        for attempt in 0..MAX_ATTEMPTS {
            let response = self
                .http
                .get(&url)
                .headers(auth_headers(&self.api_key)?)
                .query(&[("q", query), ("limit", limit_param.as_str())])
                .send()
                .await;
            let response = match response {
                Ok(response) => response,
                Err(_) if attempt + 1 < MAX_ATTEMPTS => {
                    tokio::time::sleep(retry_delay(None, attempt)).await;
                    continue;
                }
                Err(error) => return Err(error.into()),
            };
            let status = response.status();
            if should_retry(status) && attempt + 1 < MAX_ATTEMPTS {
                tokio::time::sleep(retry_delay(response.headers().get(RETRY_AFTER), attempt)).await;
                continue;
            }

            let payload: Value = response.json().await?;
            if !status.is_success() {
                return Err(anyhow!(
                    "Hermes Tweet backend returned {}: {}",
                    status.as_u16(),
                    compact_json(&payload)
                ));
            }
            return parse_tweets(&payload);
        }
        unreachable!("retry loop always returns on its final attempt")
    }
}

fn should_retry(status: reqwest::StatusCode) -> bool {
    matches!(status.as_u16(), 408 | 409 | 424 | 429) || status.is_server_error()
}

fn retry_delay(header: Option<&HeaderValue>, attempt: u32) -> Duration {
    header
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
        .map(|seconds| Duration::from_secs(seconds.min(30)))
        .unwrap_or_else(|| Duration::from_millis(250 * 2u64.pow(attempt)))
}

fn normalize_base_url(base_url: Option<&str>) -> Result<String> {
    let value = base_url.unwrap_or(DEFAULT_BASE_URL).trim();
    if value.is_empty() {
        return Ok(DEFAULT_BASE_URL.to_string());
    }
    let parsed = reqwest::Url::parse(value)?;
    let local = parsed.host_str().is_some_and(|host| {
        host.eq_ignore_ascii_case("localhost")
            || host
                .parse::<std::net::IpAddr>()
                .is_ok_and(|ip| ip.is_loopback())
    });
    if parsed.scheme() != "https" && !(parsed.scheme() == "http" && local) {
        return Err(anyhow!(
            "Hermes Tweet API base must use HTTPS; HTTP is allowed only for loopback testing"
        ));
    }
    Ok(value.trim_end_matches('/').to_string())
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

#[derive(Deserialize)]
struct SearchResponse {
    tweets: Vec<SearchTweet>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SearchTweet {
    id: String,
    #[serde(default)]
    text: String,
    #[serde(default, alias = "created_at")]
    created_at: String,
    #[serde(default, alias = "conversation_id")]
    conversation_id: String,
    #[serde(default, alias = "tweetUrl", alias = "tweet_url")]
    url: String,
    #[serde(default, alias = "like_count", alias = "likes")]
    like_count: u64,
    #[serde(default, alias = "retweet_count", alias = "retweets")]
    retweet_count: u64,
    #[serde(default, alias = "reply_count", alias = "replies")]
    reply_count: u64,
    #[serde(default, alias = "quote_count", alias = "quotes")]
    quote_count: u64,
    #[serde(default, alias = "view_count", alias = "impressions")]
    view_count: u64,
    #[serde(default, alias = "bookmark_count", alias = "bookmarks")]
    bookmark_count: u64,
    author: Option<SearchAuthor>,
}

#[derive(Deserialize)]
struct SearchAuthor {
    #[serde(default)]
    id: String,
    #[serde(default)]
    username: String,
    #[serde(default)]
    name: String,
}

fn parse_tweets(payload: &Value) -> Result<Vec<Tweet>> {
    let response: SearchResponse = serde_json::from_value(payload.clone())?;
    Ok(response.tweets.into_iter().map(normalize_tweet).collect())
}

fn normalize_tweet(tweet: SearchTweet) -> Tweet {
    let author = tweet.author.unwrap_or(SearchAuthor {
        id: String::new(),
        username: "?".to_string(),
        name: String::new(),
    });
    let username = author.username.trim_start_matches('@').to_string();
    let name = if author.name.is_empty() {
        username.clone()
    } else {
        author.name
    };
    let tweet_url = if tweet.url.is_empty() && username != "?" {
        format!("https://x.com/{username}/status/{}", tweet.id)
    } else {
        tweet.url
    };

    Tweet {
        id: tweet.id,
        text: tweet.text,
        author_id: author.id,
        username,
        name,
        created_at: tweet.created_at,
        conversation_id: tweet.conversation_id,
        metrics: TweetMetrics {
            likes: tweet.like_count,
            retweets: tweet.retweet_count,
            replies: tweet.reply_count,
            quotes: tweet.quote_count,
            impressions: tweet.view_count,
            bookmarks: tweet.bookmark_count,
        },
        urls: Vec::new(),
        mentions: Vec::new(),
        hashtags: Vec::new(),
        tweet_url,
        article: None,
        organic_metrics: None,
        non_public_metrics: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_documented_search_response() {
        let payload = serde_json::json!({
            "tweets": [{
                "id": "123",
                "text": "Hermes Agent search result",
                "author": {"id": "42", "username": "alice", "name": "Alice"},
                "createdAt": "2026-05-23T12:00:00Z",
                "conversationId": "120",
                "likeCount": 9,
                "retweetCount": 2,
                "replyCount": 3,
                "quoteCount": 4,
                "viewCount": 50,
                "bookmarkCount": 6
            }],
            "has_next_page": false,
            "next_cursor": ""
        });

        let tweets = parse_tweets(&payload).unwrap();

        assert_eq!(tweets.len(), 1);
        assert_eq!(tweets[0].id, "123");
        assert_eq!(tweets[0].author_id, "42");
        assert_eq!(tweets[0].username, "alice");
        assert_eq!(tweets[0].tweet_url, "https://x.com/alice/status/123");
        assert_eq!(tweets[0].metrics.likes, 9);
        assert_eq!(tweets[0].metrics.retweets, 2);
        assert_eq!(tweets[0].metrics.replies, 3);
        assert_eq!(tweets[0].metrics.quotes, 4);
        assert_eq!(tweets[0].metrics.impressions, 50);
        assert_eq!(tweets[0].metrics.bookmarks, 6);
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

    #[test]
    fn rejects_insecure_remote_api_base() {
        assert!(normalize_base_url(Some("http://example.com")).is_err());
        assert_eq!(
            normalize_base_url(Some("http://127.0.0.1:8080/")).unwrap(),
            "http://127.0.0.1:8080"
        );
    }

    #[test]
    fn retries_transient_statuses_and_caps_server_delay() {
        assert!(should_retry(reqwest::StatusCode::TOO_MANY_REQUESTS));
        assert!(should_retry(reqwest::StatusCode::SERVICE_UNAVAILABLE));
        assert!(!should_retry(reqwest::StatusCode::UNAUTHORIZED));
        assert_eq!(
            retry_delay(Some(&HeaderValue::from_static("120")), 0),
            Duration::from_secs(30)
        );
        assert_eq!(retry_delay(None, 2), Duration::from_secs(1));
    }
}
