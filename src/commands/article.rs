use anyhow::Result;

use crate::api::xai;
use crate::api::grok;
use crate::cli::ArticleArgs;
use crate::config::Config;
use crate::models::Article;

pub async fn run(args: &ArticleArgs, config: &Config) -> Result<()> {
    let xai_api_key = config.require_xai_key()?;

    let url = args.url.clone();

    // Check if it's an X tweet URL - warn user
    if url.contains("x.com/") && url.contains("/status/") {
        println!("📝 X tweet URLs not supported in Rust version yet.");
        println!("   Please provide a direct article URL.");
        return Ok(());
    }

    let parsed = url::Url::parse(&url)
        .map_err(|_| anyhow::anyhow!("Invalid URL: {}", url))?;
    let domain = parsed.host_str().unwrap_or("").to_string();

    let http = reqwest::Client::new();
    let raw = xai::web_search_article(
        &http,
        xai_api_key,
        &url,
        &domain,
        &args.model,
        30,
    )
    .await?;

    let article = parse_article_json(&raw, &url, &domain, args.full);

    // If AI prompt provided, analyze the article
    if let Some(ai_prompt) = &args.ai {
        println!("🤖 Analyzing with Grok...\n");
        
        let analysis = grok::analyze_query(
            &http,
            xai_api_key,
            ai_prompt,
            Some(&article.content),
            &crate::models::GrokOpts::default(),
        ).await?;
        
        println!("📝 Analysis: {}\n", ai_prompt);
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
                v.get("title").and_then(|v| v.as_str()).unwrap_or(domain).to_string(),
                v.get("description").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                v.get("content").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                v.get("author").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                v.get("published").and_then(|v| v.as_str()).unwrap_or("").to_string(),
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
        word_count: word_count,
    }
}

fn format_article(article: &Article) -> String {
    let mut out = format!("\u{1f4f0} {}\n", article.title);
    if !article.author.is_empty() {
        out.push_str(&format!("\u{270d}\u{fe0f}  {}", article.author));
    }
    if !article.published.is_empty() {
        let date = if article.published.contains('T') {
            article.published.split('T').next().unwrap_or(&article.published)
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
