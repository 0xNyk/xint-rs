use anyhow::{bail, Result};
use std::collections::{HashMap, HashSet};

use crate::client::{XClient, FIELDS};
use crate::models::*;

/// Parse raw API response into Tweet structs.
pub fn parse_tweets(raw: &RawResponse) -> Vec<Tweet> {
    let data_array = match &raw.data {
        Some(serde_json::Value::Array(arr)) => arr.clone(),
        Some(obj @ serde_json::Value::Object(_)) => vec![obj.clone()],
        _ => return Vec::new(),
    };

    let mut users: HashMap<String, (String, String)> = HashMap::new();
    if let Some(includes) = &raw.includes {
        if let Some(user_list) = &includes.users {
            for u in user_list {
                let username = u.username.clone().unwrap_or_else(|| "?".to_string());
                let name = u.name.clone().unwrap_or_else(|| "?".to_string());
                users.insert(u.id.clone(), (username, name));
            }
        }
    }

    data_array
        .iter()
        .filter_map(|t| {
            let id = t.get("id")?.as_str()?.to_string();
            let text = t.get("text")?.as_str()?.to_string();
            let author_id = t
                .get("author_id")
                .and_then(|v| v.as_str())
                .unwrap_or("?")
                .to_string();
            let created_at = t
                .get("created_at")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let conversation_id = t
                .get("conversation_id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            let (username, name) = users
                .get(&author_id)
                .cloned()
                .unwrap_or_else(|| ("?".to_string(), "?".to_string()));

            let pm = t.get("public_metrics");
            let metrics = TweetMetrics {
                likes: pm
                    .and_then(|m| m.get("like_count"))
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0),
                retweets: pm
                    .and_then(|m| m.get("retweet_count"))
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0),
                replies: pm
                    .and_then(|m| m.get("reply_count"))
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0),
                quotes: pm
                    .and_then(|m| m.get("quote_count"))
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0),
                impressions: pm
                    .and_then(|m| m.get("impression_count"))
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0),
                bookmarks: pm
                    .and_then(|m| m.get("bookmark_count"))
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0),
            };

            let entities = t.get("entities");
            let urls: Vec<UrlEntity> = entities
                .and_then(|e| e.get("urls"))
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|u| {
                            let expanded = u.get("expanded_url").and_then(|v| v.as_str())?;
                            let unwound = u.get("unwound_url").and_then(|v| v.as_str());
                            let url = unwound.unwrap_or(expanded).to_string();
                            let title = u.get("title").and_then(|v| v.as_str()).map(String::from);
                            let description = u
                                .get("description")
                                .and_then(|v| v.as_str())
                                .map(String::from);
                            let unwound_url =
                                unwound.filter(|uw| *uw != expanded).map(String::from);
                            let images = u
                                .get("images")
                                .and_then(|v| v.as_array())
                                .map(|imgs| {
                                    imgs.iter()
                                        .filter_map(|img| {
                                            img.get("url")
                                                .or_else(|| img.as_str().map(|_| img))
                                                .and_then(|v| v.as_str())
                                                .map(String::from)
                                        })
                                        .collect::<Vec<_>>()
                                })
                                .filter(|v| !v.is_empty());
                            Some(UrlEntity {
                                url,
                                title,
                                description,
                                unwound_url,
                                images,
                            })
                        })
                        .collect()
                })
                .unwrap_or_default();

            let mentions: Vec<String> = entities
                .and_then(|e| e.get("mentions"))
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|m| {
                            m.get("username").and_then(|v| v.as_str()).map(String::from)
                        })
                        .collect()
                })
                .unwrap_or_default();

            let hashtags: Vec<String> = entities
                .and_then(|e| e.get("hashtags"))
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|h| h.get("tag").and_then(|v| v.as_str()).map(String::from))
                        .collect()
                })
                .unwrap_or_default();

            let tweet_url = format!("https://x.com/{username}/status/{id}");

            // Parse inline article data if present
            let article = t.get("article").and_then(|a| {
                let plain_text = a.get("plain_text")?.as_str()?.to_string();
                if plain_text.is_empty() {
                    return None;
                }
                let title = a
                    .get("title")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let preview_text = a
                    .get("preview_text")
                    .and_then(|v| v.as_str())
                    .map(String::from);
                let cover_media = a
                    .get("cover_media")
                    .and_then(|v| v.as_str())
                    .map(String::from);
                let media_entities =
                    a.get("media_entities")
                        .and_then(|v| v.as_array())
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|v| v.as_str().map(String::from))
                                .collect()
                        });
                let entities = a.get("entities").map(|e| {
                    let code = e.get("code").and_then(|v| v.as_array()).map(|arr| {
                        arr.iter()
                            .map(|c| crate::models::TweetArticleCodeBlock {
                                language: c
                                    .get("language")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .to_string(),
                                code: c
                                    .get("code")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .to_string(),
                                content: c
                                    .get("content")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .to_string(),
                            })
                            .collect()
                    });
                    crate::models::TweetArticleEntities { code }
                });
                Some(crate::models::TweetArticle {
                    title,
                    plain_text,
                    preview_text,
                    cover_media,
                    media_entities,
                    entities,
                })
            });

            Some(Tweet {
                id,
                text,
                author_id,
                username,
                name,
                created_at,
                conversation_id,
                metrics,
                urls,
                mentions,
                hashtags,
                tweet_url,
                article,
            })
        })
        .collect()
}

/// Parse a "since" value into an ISO 8601 timestamp.
pub fn parse_since(since: &str) -> Option<String> {
    // Shorthand: "1h", "3h", "1d", "30m"
    let re = regex_lite(since);
    if let Some((num, unit)) = re {
        let ms: i64 = match unit {
            'm' => num * 60_000,
            'h' => num * 3_600_000,
            'd' => num * 86_400_000,
            _ => return None,
        };
        let start = chrono::Utc::now() - chrono::Duration::milliseconds(ms);
        return Some(start.to_rfc3339());
    }

    // ISO 8601
    if since.contains('T') || since.contains('-') {
        if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(since) {
            return Some(dt.to_rfc3339());
        }
        // Try date-only
        if let Ok(dt) = chrono::NaiveDate::parse_from_str(since, "%Y-%m-%d") {
            let dt = dt.and_hms_opt(0, 0, 0)?;
            let dt = chrono::DateTime::<chrono::Utc>::from_naive_utc_and_offset(dt, chrono::Utc);
            return Some(dt.to_rfc3339());
        }
    }

    None
}

fn regex_lite(s: &str) -> Option<(i64, char)> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    let last = s.chars().last()?;
    if !matches!(last, 'm' | 'h' | 'd' | 's') {
        return None;
    }
    let num_part = &s[..s.len() - 1];
    let num: i64 = num_part.parse().ok()?;
    Some((num, last))
}

/// Search tweets.
#[allow(clippy::too_many_arguments)]
pub async fn search(
    client: &XClient,
    token: &str,
    query: &str,
    pages: u32,
    sort_order: &str,
    since: Option<&str>,
    until: Option<&str>,
    full_archive: bool,
) -> Result<Vec<Tweet>> {
    let max_per_page = if full_archive { 500 } else { 100 };
    let encoded = urlencoding::encode(query);
    let endpoint = if full_archive {
        "tweets/search/all"
    } else {
        "tweets/search/recent"
    };

    let mut time_filter = String::new();
    if let Some(s) = since {
        if let Some(ts) = parse_since(s) {
            time_filter.push_str(&format!("&start_time={ts}"));
        }
    }
    if let Some(u) = until {
        if let Some(ts) = parse_since(u) {
            time_filter.push_str(&format!("&end_time={ts}"));
        }
    }

    let mut all_tweets = Vec::new();
    let mut next_token: Option<String> = None;

    for page in 0..pages {
        let pagination = match &next_token {
            Some(t) => format!("&next_token={t}"),
            None => String::new(),
        };
        let path = format!(
            "{endpoint}?query={encoded}&max_results={max_per_page}&{FIELDS}&sort_order={sort_order}{time_filter}{pagination}"
        );

        let raw = client.bearer_get(&path, token).await?;
        let tweets = parse_tweets(&raw);
        all_tweets.extend(tweets);

        next_token = raw.meta.and_then(|m| m.next_token);
        if next_token.is_none() {
            break;
        }
        if page < pages - 1 {
            crate::client::rate_delay().await;
        }
    }

    Ok(all_tweets)
}

/// Get a single tweet by ID.
pub async fn get_tweet(client: &XClient, token: &str, tweet_id: &str) -> Result<Option<Tweet>> {
    let path = format!("tweets/{tweet_id}?{FIELDS}");
    let raw = client.bearer_get(&path, token).await?;
    let tweets = parse_tweets(&raw);
    Ok(tweets.into_iter().next())
}

/// Fetch a full thread by conversation ID.
pub async fn get_thread(
    client: &XClient,
    token: &str,
    conversation_id: &str,
    pages: u32,
) -> Result<Vec<Tweet>> {
    let query = format!("conversation_id:{conversation_id}");
    let mut tweets = search(client, token, &query, pages, "recency", None, None, false).await?;

    // Try to fetch root tweet
    if let Ok(Some(root)) = get_tweet(client, token, conversation_id).await {
        if !tweets.iter().any(|t| t.id == root.id) {
            tweets.insert(0, root);
        }
    }

    Ok(tweets)
}

/// Get user profile + recent tweets.
pub async fn get_profile(
    client: &XClient,
    token: &str,
    username: &str,
    count: u32,
    include_replies: bool,
) -> Result<(serde_json::Value, Vec<Tweet>)> {
    let path =
        format!("users/by/username/{username}?user.fields=public_metrics,description,created_at");
    let raw = client.bearer_get(&path, token).await?;

    let user = match &raw.data {
        Some(data) => data.clone(),
        None => bail!("User @{username} not found"),
    };

    crate::client::rate_delay().await;

    let reply_filter = if include_replies { "" } else { " -is:reply" };
    let query = format!("from:{username} -is:retweet{reply_filter}");
    let tweets = search(client, token, &query, 1, "recency", None, None, false).await?;

    let tweets = tweets.into_iter().take(count as usize).collect();
    Ok((user, tweets))
}

/// Sort tweets by engagement metric.
pub fn sort_by(tweets: &mut [Tweet], metric: &str) {
    tweets.sort_by(|a, b| {
        let val = |t: &Tweet| -> u64 {
            match metric {
                "likes" => t.metrics.likes,
                "impressions" => t.metrics.impressions,
                "retweets" => t.metrics.retweets,
                "replies" => t.metrics.replies,
                _ => t.metrics.likes,
            }
        };
        val(b).cmp(&val(a))
    });
}

/// Filter by minimum engagement.
pub fn filter_engagement(tweets: Vec<Tweet>, min_likes: u64, min_impressions: u64) -> Vec<Tweet> {
    tweets
        .into_iter()
        .filter(|t| {
            (min_likes == 0 || t.metrics.likes >= min_likes)
                && (min_impressions == 0 || t.metrics.impressions >= min_impressions)
        })
        .collect()
}

/// Deduplicate tweets by ID.
pub fn dedupe(tweets: Vec<Tweet>) -> Vec<Tweet> {
    let mut seen = HashSet::new();
    tweets
        .into_iter()
        .filter(|t| seen.insert(t.id.clone()))
        .collect()
}

// urlencoding helper
mod urlencoding {
    pub fn encode(s: &str) -> String {
        let mut result = String::new();
        for c in s.chars() {
            match c {
                'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '_' | '.' | '~' => result.push(c),
                ' ' => result.push_str("%20"),
                _ => {
                    for byte in c.to_string().as_bytes() {
                        result.push_str(&format!("%{byte:02X}"));
                    }
                }
            }
        }
        result
    }
}
