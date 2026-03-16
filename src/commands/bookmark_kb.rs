use anyhow::{bail, Result};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use crate::api::{grok, twitter};
use crate::auth::oauth;
use crate::cli::BookmarkKbArgs;
use crate::client::XClient;
use crate::config::Config;
use crate::costs;
use crate::models::*;

pub async fn run(args: &BookmarkKbArgs, config: &Config, client: &XClient) -> Result<()> {
    let sub_parts = args.subcommand.as_deref().unwrap_or_default();
    let subcommand = sub_parts.first().map(|s| s.as_str()).unwrap_or("help");
    let sub_rest: Vec<String> = sub_parts.iter().skip(1).cloned().collect();

    match subcommand {
        "extract" => run_extract(args, config, client).await,
        "search" => run_search(args, &sub_rest, config).await,
        "sync" => run_sync(args, config).await,
        "topics" => run_topics(args, config),
        "status" => run_status(args, config),
        "help" | "--help" | "-h" => {
            print_help();
            Ok(())
        }
        _ => {
            eprintln!("Unknown subcommand: {subcommand}");
            print_help();
            Ok(())
        }
    }
}

fn kb_path(config: &Config) -> PathBuf {
    config.data_dir.join("bookmark-kb.json")
}

fn load_kb(config: &Config) -> BookmarkKnowledgeBase {
    let path = kb_path(config);
    if path.exists() {
        if let Ok(data) = fs::read_to_string(&path) {
            if let Ok(kb) = serde_json::from_str(&data) {
                return kb;
            }
        }
    }
    BookmarkKnowledgeBase::default()
}

fn save_kb(config: &Config, kb: &BookmarkKnowledgeBase) -> Result<()> {
    let path = kb_path(config);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(kb)?;
    fs::write(&path, json)?;
    Ok(())
}

async fn run_extract(args: &BookmarkKbArgs, config: &Config, client: &XClient) -> Result<()> {
    let client_id = config.require_client_id()?;
    let (access_token, tokens) =
        oauth::get_valid_token(client, &config.tokens_path(), client_id).await?;

    eprintln!("Fetching bookmarks for @{}...", tokens.username);
    let mut all_tweets = Vec::new();
    let mut next_token: Option<String> = None;
    let per_page = args.limit.min(100);
    let max_pages = args.limit.div_ceil(per_page).min(8);

    for page in 0..max_pages {
        let pagination = match &next_token {
            Some(t) => format!("&pagination_token={t}"),
            None => String::new(),
        };
        let path = format!(
            "users/{}/bookmarks?max_results={}&{}{}",
            tokens.user_id,
            per_page,
            crate::client::FIELDS,
            pagination
        );
        let raw = client.oauth_get(&path, &access_token).await?;
        let tweets = twitter::parse_tweets(&raw);
        all_tweets.extend(tweets);
        if all_tweets.len() >= args.limit {
            break;
        }
        next_token = raw.meta.and_then(|m| m.next_token);
        if next_token.is_none() {
            break;
        }
        if page < max_pages - 1 {
            crate::client::rate_delay().await;
        }
    }
    all_tweets.truncate(args.limit);

    costs::track_cost(
        &config.costs_path(),
        "bookmarks",
        "/2/users/bookmarks",
        all_tweets.len() as u64,
    );

    if all_tweets.is_empty() {
        println!("No bookmarks found.");
        return Ok(());
    }

    let mut kb = load_kb(config);
    let existing_ids: std::collections::HashSet<&str> = if args.force {
        std::collections::HashSet::new()
    } else {
        kb.extractions.iter().map(|e| e.tweet_id.as_str()).collect()
    };

    let new_tweets: Vec<&Tweet> = all_tweets
        .iter()
        .filter(|t| !existing_ids.contains(t.id.as_str()))
        .collect();

    if new_tweets.is_empty() {
        eprintln!(
            "All {} bookmarks already extracted. Use --force to re-extract.",
            all_tweets.len()
        );
        if args.json {
            println!("{}", serde_json::to_string_pretty(&kb)?);
        }
        return Ok(());
    }

    eprintln!(
        "Extracting knowledge from {} new bookmarks ({} already processed)...",
        new_tweets.len(),
        existing_ids.len()
    );

    let existing_topics: Vec<String> = kb.topic_index.keys().cloned().collect();

    let api_key = config.require_xai_key()?;
    let http = reqwest::Client::new();
    let batches: Vec<Vec<&Tweet>> = new_tweets
        .chunks(args.batch_size)
        .map(|c| c.to_vec())
        .collect();

    for (i, batch) in batches.iter().enumerate() {
        eprintln!(
            "  Batch {}/{} ({} tweets)...",
            i + 1,
            batches.len(),
            batch.len()
        );

        let tweet_context: String = batch
            .iter()
            .map(|t| {
                let links: String = t
                    .urls
                    .iter()
                    .map(|u| {
                        let expanded = u.unwound_url.as_deref().unwrap_or(&u.url);
                        let title = u
                            .title
                            .as_deref()
                            .map(|t| format!(" ({t})"))
                            .unwrap_or_default();
                        format!("  - {expanded}{title}")
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                let links_section = if links.is_empty() {
                    String::new()
                } else {
                    format!("\nLinks:\n{links}")
                };
                format!(
                    "Tweet {} by @{} ({}):\n{}{}\n---",
                    t.id, t.username, t.tweet_url, t.text, links_section
                )
            })
            .collect::<Vec<_>>()
            .join("\n\n");

        let topics_hint = if existing_topics.is_empty() {
            "none yet".to_string()
        } else {
            existing_topics.join(", ")
        };

        let system_prompt = format!(
            "You are a knowledge extraction engine. Given these tweets, extract structured knowledge for each one.\n\n\
            For each tweet, output:\n\
            - topics: 2-5 categories (use consistent naming, prefer existing topics: {topics_hint})\n\
            - entities: people, companies, products, technologies mentioned\n\
            - summary: 1-2 sentence summary of the key information\n\
            - evaluation: 1-2 sentence assessment of why this bookmark is valuable, what makes it worth saving, and how it could be applied or referenced later\n\
            - sentiment: positive/negative/neutral/mixed\n\
            - importance: 1-5 (5 = highly actionable/valuable)\n\
            - key_insights: 1-3 bullet points of the most valuable takeaways\n\n\
            Return a JSON array of objects with fields: tweet_id, topics, entities, summary, evaluation, sentiment, importance, key_insights\n\n\
            IMPORTANT: Return ONLY valid JSON. No markdown fences, no explanation."
        );

        let messages = vec![
            GrokMessage {
                role: "system".to_string(),
                content: system_prompt,
            },
            GrokMessage {
                role: "user".to_string(),
                content: tweet_context,
            },
        ];

        let opts = GrokOpts {
            model: args.model.clone(),
            temperature: 0.3,
            max_tokens: 2048,
        };

        let response =
            grok::grok_chat_tracked(&http, api_key, &messages, &opts, Some(&config.costs_path()))
                .await?;

        let content = response.content.trim();
        let json_str = if content.starts_with("```") {
            let stripped = content
                .trim_start_matches("```json")
                .trim_start_matches("```");
            stripped.trim_end_matches("```").trim()
        } else {
            content
        };

        match serde_json::from_str::<Vec<serde_json::Value>>(json_str) {
            Ok(items) => {
                for item in &items {
                    let tweet_id = item
                        .get("tweet_id")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let tweet = batch.iter().find(|t| t.id == tweet_id);

                    let extraction = BookmarkExtraction {
                        tweet_id: tweet_id.clone(),
                        tweet_url: tweet.map(|t| t.tweet_url.clone()).unwrap_or_default(),
                        author: tweet.map(|t| t.username.clone()).unwrap_or_default(),
                        text_preview: tweet
                            .map(|t| t.text.chars().take(200).collect())
                            .unwrap_or_default(),
                        topics: item
                            .get("topics")
                            .and_then(|v| serde_json::from_value(v.clone()).ok())
                            .unwrap_or_default(),
                        entities: item
                            .get("entities")
                            .and_then(|v| serde_json::from_value(v.clone()).ok())
                            .unwrap_or_default(),
                        summary: item
                            .get("summary")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string(),
                        evaluation: item
                            .get("evaluation")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string(),
                        sentiment: item
                            .get("sentiment")
                            .and_then(|v| v.as_str())
                            .unwrap_or("neutral")
                            .to_string(),
                        importance: item.get("importance").and_then(|v| v.as_u64()).unwrap_or(3)
                            as u8,
                        key_insights: item
                            .get("key_insights")
                            .and_then(|v| serde_json::from_value(v.clone()).ok())
                            .unwrap_or_default(),
                        source_links: tweet
                            .map(|t| {
                                t.urls
                                    .iter()
                                    .map(|u| {
                                        let expanded =
                                            u.unwound_url.as_deref().unwrap_or(&u.url).to_string();
                                        let domain = expanded
                                            .replace("https://", "")
                                            .replace("http://", "")
                                            .split('/')
                                            .next()
                                            .unwrap_or("")
                                            .to_string();
                                        SourceLink {
                                            url: expanded,
                                            title: u.title.clone(),
                                            domain: if domain.is_empty() {
                                                None
                                            } else {
                                                Some(domain)
                                            },
                                        }
                                    })
                                    .collect()
                            })
                            .unwrap_or_default(),
                        urls: tweet
                            .map(|t| {
                                t.urls
                                    .iter()
                                    .map(|u| u.unwound_url.as_deref().unwrap_or(&u.url).to_string())
                                    .collect()
                            })
                            .unwrap_or_default(),
                        extracted_at: chrono::Utc::now().to_rfc3339(),
                    };

                    if args.force {
                        kb.extractions.retain(|e| e.tweet_id != extraction.tweet_id);
                    }
                    kb.extractions.push(extraction);
                }
            }
            Err(e) => {
                eprintln!("  Warning: Failed to parse batch {} response: {}", i + 1, e);
                eprintln!(
                    "  Raw response: {}...",
                    &json_str[..json_str.len().min(200)]
                );
            }
        }
    }

    kb.topic_index.clear();
    for ext in &kb.extractions {
        for topic in &ext.topics {
            kb.topic_index
                .entry(topic.clone())
                .or_default()
                .push(ext.tweet_id.clone());
        }
    }

    kb.last_extracted = chrono::Utc::now().to_rfc3339();
    kb.total_bookmarks_processed = kb.extractions.len();

    save_kb(config, &kb)?;

    if args.json {
        println!("{}", serde_json::to_string_pretty(&kb)?);
    } else {
        let new_count = new_tweets.len();
        println!("\nBookmark Knowledge Extraction Complete");
        println!("  New extractions: {}", new_count);
        println!("  Total in KB: {}", kb.extractions.len());
        println!("  Topics discovered: {}", kb.topic_index.len());
        if !kb.topic_index.is_empty() {
            println!("\n  Top topics:");
            let mut topics: Vec<_> = kb.topic_index.iter().collect();
            topics.sort_by(|a, b| b.1.len().cmp(&a.1.len()));
            for (topic, ids) in topics.iter().take(10) {
                println!("    {}: {} tweets", topic, ids.len());
            }
        }
    }

    Ok(())
}

async fn run_search(args: &BookmarkKbArgs, query_parts: &[String], config: &Config) -> Result<()> {
    let query = query_parts.join(" ");
    if query.is_empty() {
        bail!("Search query required. Usage: xint bookmark-kb search \"your query\"");
    }

    let kb = load_kb(config);
    if kb.extractions.is_empty() {
        println!("Knowledge base is empty. Run 'xint bookmark-kb extract' first.");
        return Ok(());
    }

    if args.remote {
        if let Some(ref cid) = kb.collection_id {
            let api_key = config.require_xai_key()?;
            let http = reqwest::Client::new();
            let body = serde_json::json!({
                "query": query,
                "top_k": args.limit.min(20),
                "collection_ids": [cid],
            });
            let res = http
                .post("https://api.x.ai/v1/documents/search")
                .header("Authorization", format!("Bearer {api_key}"))
                .header("Content-Type", "application/json")
                .json(&body)
                .send()
                .await?;
            if !res.status().is_success() {
                let text = res.text().await.unwrap_or_default();
                bail!("xAI search error: {}", &text[..text.len().min(300)]);
            }
            let data: serde_json::Value = res.json().await?;
            println!("{}", serde_json::to_string_pretty(&data)?);
            return Ok(());
        } else {
            eprintln!("No collection synced yet. Run 'xint bookmark-kb sync' first. Falling back to local search.");
        }
    }

    let q_lower = query.to_lowercase();
    let limit = args.limit.min(50);

    let mut scored: Vec<(usize, &BookmarkExtraction)> = kb
        .extractions
        .iter()
        .filter_map(|ext| {
            if let Some(ref topic_filter) = args.topic {
                if !ext
                    .topics
                    .iter()
                    .any(|t| t.to_lowercase().contains(&topic_filter.to_lowercase()))
                {
                    return None;
                }
            }
            if let Some(min_imp) = args.min_importance {
                if ext.importance < min_imp {
                    return None;
                }
            }

            let mut score = 0usize;
            if ext.summary.to_lowercase().contains(&q_lower) {
                score += 3;
            }
            for insight in &ext.key_insights {
                if insight.to_lowercase().contains(&q_lower) {
                    score += 2;
                }
            }
            for topic in &ext.topics {
                if topic.to_lowercase().contains(&q_lower) {
                    score += 2;
                }
            }
            for entity in &ext.entities {
                if entity.to_lowercase().contains(&q_lower) {
                    score += 1;
                }
            }
            if ext.text_preview.to_lowercase().contains(&q_lower) {
                score += 1;
            }
            if ext.author.to_lowercase().contains(&q_lower) {
                score += 1;
            }
            if ext.evaluation.to_lowercase().contains(&q_lower) {
                score += 2;
            }

            if score > 0 {
                Some((score, ext))
            } else {
                None
            }
        })
        .collect();

    scored.sort_by(|a, b| b.0.cmp(&a.0));
    let results: Vec<_> = scored.into_iter().take(limit).collect();

    if results.is_empty() {
        println!("No results found for \"{}\".", query);
        return Ok(());
    }

    if args.json {
        let json_results: Vec<_> = results
            .iter()
            .map(|(score, ext)| serde_json::json!({ "score": score, "extraction": ext }))
            .collect();
        println!("{}", serde_json::to_string_pretty(&json_results)?);
    } else {
        println!(
            "\nSearch results for \"{}\" ({} matches):\n",
            query,
            results.len()
        );
        for (score, ext) in &results {
            println!("  [{}/10] @{} — {}", score, ext.author, ext.summary);
            if !ext.evaluation.is_empty() {
                println!("    Evaluation: {}", ext.evaluation);
            }
            if !ext.key_insights.is_empty() {
                for insight in &ext.key_insights {
                    println!("    - {insight}");
                }
            }
            if !ext.source_links.is_empty() {
                println!("    Sources:");
                for link in &ext.source_links {
                    let label = match &link.title {
                        Some(t) => format!("{t} ({})", link.domain.as_deref().unwrap_or("")),
                        None => link.url.clone(),
                    };
                    println!("      {label}");
                }
            }
            println!(
                "    Topics: {} | Importance: {}/5",
                ext.topics.join(", "),
                ext.importance
            );
            println!("    {}", ext.tweet_url);
            println!();
        }
    }

    Ok(())
}

async fn run_sync(args: &BookmarkKbArgs, config: &Config) -> Result<()> {
    let mut kb = load_kb(config);
    if kb.extractions.is_empty() {
        println!("Knowledge base is empty. Run 'xint bookmark-kb extract' first.");
        return Ok(());
    }

    let cloud = args
        .subcommand
        .as_ref()
        .map(|v| v.iter().any(|s| s == "--cloud"))
        .unwrap_or(false);

    let exports_dir = config.data_dir.join("exports").join("bookmark-kb");
    fs::create_dir_all(&exports_dir)?;

    let mut topic_groups: HashMap<String, Vec<&BookmarkExtraction>> = HashMap::new();
    for ext in &kb.extractions {
        for topic in &ext.topics {
            topic_groups.entry(topic.clone()).or_default().push(ext);
        }
    }

    for (topic, extractions) in &topic_groups {
        let safe_name = topic.to_lowercase().replace(['/', ' ', '\\'], "-");
        let filename = format!("{safe_name}.md");
        let filepath = exports_dir.join(&filename);

        let mut md = format!("# {topic}\n\n");
        for ext in extractions {
            md.push_str(&format!("## @{} — {}\n\n", ext.author, ext.summary));
            if !ext.evaluation.is_empty() {
                md.push_str(&format!("> {}\n\n", ext.evaluation));
            }
            for insight in &ext.key_insights {
                md.push_str(&format!("- {insight}\n"));
            }
            md.push_str(&format!(
                "\nImportance: {}/5 | Sentiment: {}\n",
                ext.importance, ext.sentiment
            ));
            if !ext.source_links.is_empty() {
                md.push_str("Links:\n");
                for link in &ext.source_links {
                    if let Some(ref title) = link.title {
                        md.push_str(&format!("  - [{title}]({})\n", link.url));
                    } else {
                        md.push_str(&format!("  - {}\n", link.url));
                    }
                }
            }
            md.push_str(&format!("Source: {}\n\n---\n\n", ext.tweet_url));
        }

        fs::write(&filepath, &md)?;
        eprintln!("  Exported: {filename} ({} entries)", extractions.len());
    }

    println!(
        "\nExported {} topic files to {}",
        topic_groups.len(),
        exports_dir.display()
    );

    if cloud {
        let api_key = config.require_xai_key()?;
        let mgmt_key = config.require_xai_management_key()?;
        let http = reqwest::Client::new();

        eprintln!("Ensuring collection '{}'...", args.collection_name);

        let list_res = http
            .get("https://management-api.x.ai/v1/collections")
            .header("Authorization", format!("Bearer {mgmt_key}"))
            .send()
            .await?;
        if !list_res.status().is_success() {
            bail!("Failed to list collections");
        }
        let list_data: serde_json::Value = list_res.json().await?;
        let collections = list_data.get("data").and_then(|v| v.as_array());

        let collection_id = if let Some(existing) = collections.and_then(|arr| {
            arr.iter()
                .find(|c| c.get("name").and_then(|v| v.as_str()) == Some(&args.collection_name))
        }) {
            let id = existing
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            eprintln!("  Using existing collection: {}", args.collection_name);
            id
        } else {
            let create_res = http
                .post("https://management-api.x.ai/v1/collections")
                .header("Authorization", format!("Bearer {mgmt_key}"))
                .header("Content-Type", "application/json")
                .json(&serde_json::json!({
                    "name": &args.collection_name,
                    "description": "Bookmark knowledge base"
                }))
                .send()
                .await?;
            if !create_res.status().is_success() {
                let text = create_res.text().await.unwrap_or_default();
                bail!(
                    "Failed to create collection: {}",
                    &text[..text.len().min(300)]
                );
            }
            let create_data: serde_json::Value = create_res.json().await?;
            let id = create_data
                .pointer("/collection/id")
                .or_else(|| create_data.pointer("/data/id"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            eprintln!("  Created new collection: {}", args.collection_name);
            id
        };

        let mut uploaded = 0;
        let mut attached = 0;

        for topic in topic_groups.keys() {
            let safe_name = topic.to_lowercase().replace(['/', ' ', '\\'], "-");
            let filename = format!("{safe_name}.md");
            let filepath = exports_dir.join(&filename);

            let file_bytes = fs::read(&filepath)?;
            let form = reqwest::multipart::Form::new()
                .part(
                    "file",
                    reqwest::multipart::Part::bytes(file_bytes)
                        .file_name(filename.clone())
                        .mime_str("text/markdown")?,
                )
                .text("purpose", "kb_sync");

            let upload_res = http
                .post("https://api.x.ai/v1/files")
                .header("Authorization", format!("Bearer {api_key}"))
                .multipart(form)
                .send()
                .await?;

            if upload_res.status().is_success() {
                uploaded += 1;
                let upload_data: serde_json::Value = upload_res.json().await?;
                if let Some(file_id) = upload_data
                    .pointer("/data/id")
                    .or_else(|| upload_data.get("id"))
                    .and_then(|v| v.as_str())
                {
                    let attach_res = http
                        .post(format!(
                            "https://management-api.x.ai/v1/collections/{collection_id}/documents"
                        ))
                        .header("Authorization", format!("Bearer {mgmt_key}"))
                        .header("Content-Type", "application/json")
                        .json(&serde_json::json!({ "document_id": file_id }))
                        .send()
                        .await?;
                    if attach_res.status().is_success() {
                        attached += 1;
                    } else {
                        eprintln!("    Warning: Failed to attach {filename} to collection");
                    }
                }
            } else {
                eprintln!("    Warning: Failed to upload {filename}");
            }
        }

        kb.collection_id = Some(collection_id);
        kb.last_synced = Some(chrono::Utc::now().to_rfc3339());
        save_kb(config, &kb)?;

        println!(
            "Uploaded {uploaded}/{} topics to collection (attached: {attached})",
            topic_groups.len()
        );
    }

    Ok(())
}

fn run_topics(args: &BookmarkKbArgs, config: &Config) -> Result<()> {
    let kb = load_kb(config);
    if kb.topic_index.is_empty() {
        println!("No topics yet. Run 'xint bookmark-kb extract' first.");
        return Ok(());
    }

    let mut topics: Vec<_> = kb.topic_index.iter().collect();
    topics.sort_by(|a, b| b.1.len().cmp(&a.1.len()));

    if args.json {
        let arr: Vec<serde_json::Value> = topics
            .iter()
            .map(|(topic, ids)| {
                serde_json::json!({
                    "topic": topic,
                    "count": ids.len(),
                    "tweet_ids": ids,
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&arr)?);
        return Ok(());
    }

    println!("\nBookmark Topics ({} total):\n", topics.len());
    println!("  {:<30} Count", "Topic");
    println!("  {:<30} -----", "-----");
    for (topic, ids) in &topics {
        println!("  {:<30} {}", topic, ids.len());
    }

    Ok(())
}

fn run_status(args: &BookmarkKbArgs, config: &Config) -> Result<()> {
    let kb = load_kb(config);

    if args.json {
        let status = serde_json::json!({
            "extractions": kb.extractions.len(),
            "topics": kb.topic_index.len(),
            "last_extracted": if kb.last_extracted.is_empty() { None } else { Some(&kb.last_extracted) },
            "collection_id": kb.collection_id,
            "last_synced": kb.last_synced,
            "kb_file": kb_path(config).display().to_string(),
        });
        println!("{}", serde_json::to_string_pretty(&status)?);
        return Ok(());
    }

    println!("\nBookmark Knowledge Base Status:\n");
    println!("  Total extractions: {}", kb.extractions.len());
    println!("  Topics discovered: {}", kb.topic_index.len());
    if !kb.last_extracted.is_empty() {
        println!("  Last extracted: {}", kb.last_extracted);
    } else {
        println!("  Last extracted: never");
    }
    if let Some(ref cid) = kb.collection_id {
        println!("  Collection ID: {}", cid);
        println!(
            "  Last synced: {}",
            kb.last_synced.as_deref().unwrap_or("never")
        );
    } else {
        println!("  Sync status: not synced");
    }
    println!("  KB file: {}", kb_path(config).display());

    Ok(())
}

fn print_help() {
    println!(
        "
Bookmark Knowledge Base — Extract and search knowledge from your bookmarks

Usage: xint bookmark-kb <subcommand> [options]

Subcommands:
  extract              Fetch bookmarks and extract knowledge via Grok AI
  search <query>       Search local knowledge base
  sync                 Export knowledge as local markdown files
  topics               List all discovered topics with counts
  status               Show KB stats

Extract options:
  --limit <N>          Max bookmarks to process (default: 100)
  --since <dur>        Only bookmarks from this time window (e.g. 7d)
  --batch-size <N>     Tweets per Grok call (default: 20)
  --model <name>       Grok model (default: grok-4-1-fast)
  --force              Re-extract already-processed tweets
  --json               JSON output

Search options:
  --topic <name>       Filter by topic
  --min-importance <N> Minimum importance (1-5)
  --remote             Search via xAI Collections (semantic)
  --json               JSON output

Sync options:
  --cloud                   Also upload to xAI Collections (requires management key)
  --collection-name <name>  Collection name (default: xint-bookmarks)

Examples:
  xint bookmark-kb extract --limit 50
  xint bookmark-kb search \"AI agents\" --topic \"AI/ML\"
  xint bookmark-kb sync
  xint bookmark-kb topics
"
    );
}
