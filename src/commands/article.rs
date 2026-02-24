use anyhow::Result;

use crate::api::grok;
use crate::api::twitter;
use crate::api::xai;
use crate::cli::ArticleArgs;
use crate::client::XClient;
use crate::config::Config;
use crate::models::{Article, Tweet, TweetArticle};

pub async fn run(args: &ArticleArgs, config: &Config) -> Result<()> {
    let mut url = args.url.clone();
    let mut article: Option<Article> = None;

    if is_x_tweet_like_url(&url) {
        println!("🔍 Fetching tweet to extract linked article...");
        let token = config.require_bearer_token()?;
        let client = XClient::new()?;
        let (tweet, article_url, inline_article) =
            fetch_tweet_for_article(&client, token, &url).await?;

        if let Some(inline) = inline_article {
            println!("📄 Found X Article: {}\n", inline.title);
            article = Some(inline);
        } else if let Some(found) = article_url {
            println!("📝 Tweet: {}", tweet.tweet_url);
            println!("📄 Found link: {found}\n");
            url = found;
        } else {
            println!("📝 No external link found in tweet.");
            println!("   Tweet: {}", tweet.tweet_url);
            return Ok(());
        }
    }

    let article = if let Some(a) = article {
        a
    } else {
        let xai_api_key = config.require_xai_key()?;
        let parsed = url::Url::parse(&url).map_err(|_| anyhow::anyhow!("Invalid URL: {url}"))?;
        let domain = parsed.host_str().unwrap_or("").to_string();
        let timeout_secs = resolve_article_timeout_secs();

        let http = reqwest::Client::new();
        let raw =
            xai::web_search_article(&http, xai_api_key, &url, &domain, &args.model, timeout_secs)
                .await?;
        parse_article_json(&raw, &url, &domain, args.full)
    };

    // If AI prompt provided, analyze the article
    if let Some(ai_prompt) = &args.ai {
        println!("🤖 Analyzing with Grok...\n");

        let xai_key = config.require_xai_key()?;
        let http = reqwest::Client::new();
        let analysis = grok::analyze_query(
            &http,
            xai_key,
            ai_prompt,
            Some(&article.content),
            &crate::models::GrokOpts::default(),
        )
        .await?;

        println!("📝 Analysis: {ai_prompt}\n");
        println!("{}", analysis.content);
        println!("\n---");
    }

    if args.json {
        println!("{}", serde_json::to_string_pretty(&article)?);
    } else {
        println!("{}", format_article(&article));
    }

    Ok(())
}

fn is_x_tweet_like_url(value: &str) -> bool {
    extract_tweet_id(value).is_some()
}

fn resolve_article_timeout_secs() -> u64 {
    const DEFAULT_TIMEOUT_SECS: u64 = 30;
    let parsed = std::env::var("XINT_ARTICLE_TIMEOUT_SEC")
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .unwrap_or(DEFAULT_TIMEOUT_SECS);
    parsed.clamp(5, 120)
}

fn extract_tweet_id(input: &str) -> Option<String> {
    let trimmed = input.trim();
    if trimmed.chars().all(|c| c.is_ascii_digit()) {
        return Some(trimmed.to_string());
    }

    let status_idx = trimmed.find("/status/")?;
    let id_part = &trimmed[status_idx + "/status/".len()..];
    let digits: String = id_part.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        None
    } else {
        Some(digits)
    }
}

fn is_x_article_url(raw: &str) -> bool {
    if let Ok(url) = url::Url::parse(raw) {
        let host = url.host_str().unwrap_or_default();
        return host.eq_ignore_ascii_case("x.com") && url.path().starts_with("/i/article/");
    }
    false
}

fn is_external_non_x_url(raw: &str) -> bool {
    if let Ok(url) = url::Url::parse(raw) {
        let host = url.host_str().unwrap_or_default().to_ascii_lowercase();
        return host != "x.com"
            && host != "twitter.com"
            && !host.ends_with(".x.com")
            && !host.ends_with(".twitter.com");
    }
    false
}

fn pick_article_url_from_tweet(tweet: &Tweet) -> Option<String> {
    let mut candidates: Vec<String> = Vec::new();
    for item in &tweet.urls {
        if !item.url.is_empty() {
            candidates.push(item.url.clone());
        }
        if let Some(unwound) = &item.unwound_url {
            if !unwound.is_empty() {
                candidates.push(unwound.clone());
            }
        }
    }

    candidates.sort();
    candidates.dedup();

    if let Some(url) = candidates.iter().find(|url| is_external_non_x_url(url)) {
        return Some(url.clone());
    }
    if let Some(url) = candidates.iter().find(|url| is_x_article_url(url)) {
        return Some(url.clone());
    }
    candidates.into_iter().next()
}

async fn fetch_tweet_for_article(
    client: &XClient,
    token: &str,
    tweet_url: &str,
) -> Result<(Tweet, Option<String>, Option<Article>)> {
    let tweet_id = extract_tweet_id(tweet_url)
        .ok_or_else(|| anyhow::anyhow!("Invalid X tweet URL: {tweet_url}"))?;
    let tweet = twitter::get_tweet(client, token, &tweet_id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("Tweet not found: {tweet_id}"))?;

    // If tweet has inline article data from X API, build Article directly
    if let Some(ref tw_article) = tweet.article {
        if !tw_article.plain_text.is_empty() {
            let content = reconstruct_article_content(tw_article);
            let word_count = content.split_whitespace().count() as u64;
            let ttr = (word_count as f64 / 238.0).ceil().max(1.0) as u64;
            let inline = Article {
                url: tweet_url.to_string(),
                title: if tw_article.title.is_empty() {
                    "X Article".to_string()
                } else {
                    tw_article.title.clone()
                },
                description: tw_article.preview_text.clone().unwrap_or_default(),
                content,
                author: tweet.username.clone(),
                published: tweet.created_at.clone(),
                domain: "x.com".to_string(),
                ttr,
                word_count,
            };
            return Ok((tweet, None, Some(inline)));
        }
    }

    let article_url = pick_article_url_from_tweet(&tweet);
    Ok((tweet, article_url, None))
}

fn reconstruct_article_content(article: &TweetArticle) -> String {
    let mut content = article.plain_text.clone();
    if let Some(ref entities) = article.entities {
        if let Some(ref code_blocks) = entities.code {
            if !code_blocks.is_empty() {
                content.push_str("\n\n---\n\nCode examples from article:\n");
                for block in code_blocks {
                    content.push_str(&format!("\n{}\n", block.content));
                }
            }
        }
    }
    content
}

fn parse_article_json(raw: &str, url: &str, domain: &str, full: bool) -> Article {
    // Strip markdown fences if present
    let mut cleaned = raw.trim().to_string();
    if cleaned.starts_with("```") {
        if let Some(start) = cleaned.find('\n') {
            cleaned = cleaned[start + 1..].to_string();
        }
        if cleaned.ends_with("```") {
            cleaned = cleaned[..cleaned.len() - 3].trim().to_string();
        }
    }

    let (title, description, mut content, author, published) =
        match serde_json::from_str::<serde_json::Value>(&cleaned) {
            Ok(v) => (
                v.get("title")
                    .and_then(|v| v.as_str())
                    .unwrap_or(domain)
                    .to_string(),
                v.get("description")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                v.get("content")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                v.get("author")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                v.get("published")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
            ),
            Err(_) => (
                domain.to_string(),
                String::new(),
                cleaned,
                String::new(),
                String::new(),
            ),
        };

    let word_count = content.split_whitespace().count() as u64;
    let ttr = (word_count as f64 / 238.0).ceil() as u64;

    if !full && content.len() > 5000 {
        // Truncate at word boundary
        let truncated = &content[..5000];
        let end = truncated.rfind(char::is_whitespace).unwrap_or(5000);
        content = format!("{}\n\n[... truncated]", &content[..end]);
    }

    Article {
        url: url.to_string(),
        title,
        description,
        content,
        author,
        published,
        domain: domain.to_string(),
        ttr,
        word_count,
    }
}

fn format_article(article: &Article) -> String {
    let mut out = format!("\u{1f4f0} {}\n", article.title);
    if !article.author.is_empty() {
        out.push_str(&format!("\u{270d}\u{fe0f}  {}", article.author));
    }
    if !article.published.is_empty() {
        let date = if article.published.contains('T') {
            article
                .published
                .split('T')
                .next()
                .unwrap_or(&article.published)
        } else {
            &article.published
        };
        if !out.ends_with('\n') {
            out.push_str(" \u{00b7} ");
        }
        out.push_str(date);
    }
    if !article.author.is_empty() || !article.published.is_empty() {
        out.push('\n');
    }
    out.push_str(&format!("\u{1f517} {}\n", article.url));
    out.push_str(&format!(
        "\u{1f4ca} {} words \u{00b7} {} min read\n",
        article.word_count, article.ttr
    ));
    if !article.description.is_empty() {
        out.push_str(&format!("\n{}\n", article.description));
    }
    out.push_str(&format!("\n---\n\n{}", article.content));
    out
}

#[cfg(test)]
mod tests {
    use super::{extract_tweet_id, pick_article_url_from_tweet, resolve_article_timeout_secs};
    use crate::models::{Tweet, TweetMetrics, UrlEntity};

    fn fake_tweet(urls: Vec<UrlEntity>) -> Tweet {
        Tweet {
            id: "1900100012345678901".to_string(),
            text: "tweet".to_string(),
            author_id: "1".to_string(),
            username: "alice".to_string(),
            name: "Alice".to_string(),
            created_at: "2026-02-19T00:00:00Z".to_string(),
            conversation_id: "1900100012345678901".to_string(),
            metrics: TweetMetrics {
                likes: 0,
                retweets: 0,
                replies: 0,
                quotes: 0,
                impressions: 0,
                bookmarks: 0,
            },
            urls,
            mentions: vec![],
            hashtags: vec![],
            tweet_url: "https://x.com/alice/status/1900100012345678901".to_string(),
            article: None,
        }
    }

    #[test]
    fn extract_tweet_id_supports_status_urls() {
        assert_eq!(
            extract_tweet_id("https://x.com/user/status/1900100012345678901"),
            Some("1900100012345678901".to_string())
        );
        assert_eq!(
            extract_tweet_id("https://twitter.com/i/web/status/1900100012345678901"),
            Some("1900100012345678901".to_string())
        );
    }

    #[test]
    fn pick_article_url_prefers_external_link() {
        let tweet = fake_tweet(vec![
            UrlEntity {
                url: "https://x.com/i/article/abc".to_string(),
                title: None,
                description: None,
                unwound_url: None,
                images: None,
            },
            UrlEntity {
                url: "https://example.com/deep-dive".to_string(),
                title: None,
                description: None,
                unwound_url: None,
                images: None,
            },
        ]);
        assert_eq!(
            pick_article_url_from_tweet(&tweet),
            Some("https://example.com/deep-dive".to_string())
        );
    }

    #[test]
    fn pick_article_url_falls_back_to_x_article() {
        let tweet = fake_tweet(vec![UrlEntity {
            url: "https://x.com/i/article/xyz".to_string(),
            title: None,
            description: None,
            unwound_url: None,
            images: None,
        }]);
        assert_eq!(
            pick_article_url_from_tweet(&tweet),
            Some("https://x.com/i/article/xyz".to_string())
        );
    }

    #[test]
    fn reconstruct_article_content_plain_text_only() {
        use crate::models::TweetArticle;
        let article = TweetArticle {
            title: "Test".to_string(),
            plain_text: "Hello world article.".to_string(),
            preview_text: None,
            cover_media: None,
            media_entities: None,
            entities: None,
        };
        assert_eq!(
            super::reconstruct_article_content(&article),
            "Hello world article."
        );
    }

    #[test]
    fn reconstruct_article_content_with_code_blocks() {
        use crate::models::{TweetArticle, TweetArticleCodeBlock, TweetArticleEntities};
        let article = TweetArticle {
            title: "Test".to_string(),
            plain_text: "Some text.".to_string(),
            preview_text: None,
            cover_media: None,
            media_entities: None,
            entities: Some(TweetArticleEntities {
                code: Some(vec![TweetArticleCodeBlock {
                    language: "rust".to_string(),
                    code: "let x = 1;".to_string(),
                    content: "```rust\nlet x = 1;\n```".to_string(),
                }]),
            }),
        };
        let result = super::reconstruct_article_content(&article);
        assert!(result.contains("Some text."));
        assert!(result.contains("Code examples from article:"));
        assert!(result.contains("```rust"));
    }

    #[test]
    fn article_timeout_defaults_and_clamps() {
        std::env::remove_var("XINT_ARTICLE_TIMEOUT_SEC");
        assert_eq!(resolve_article_timeout_secs(), 30);

        std::env::set_var("XINT_ARTICLE_TIMEOUT_SEC", "1");
        assert_eq!(resolve_article_timeout_secs(), 5);

        std::env::set_var("XINT_ARTICLE_TIMEOUT_SEC", "999");
        assert_eq!(resolve_article_timeout_secs(), 120);

        std::env::remove_var("XINT_ARTICLE_TIMEOUT_SEC");
    }
}
