use anyhow::Result;
use std::collections::HashMap;

use crate::api::twitter;
use crate::cli::TrendsArgs;
use crate::client::XClient;
use crate::config::Config;
use crate::costs;
use crate::models::*;

fn woeid_map() -> HashMap<&'static str, u32> {
    let mut m = HashMap::new();
    m.insert("worldwide", 1);
    m.insert("world", 1);
    m.insert("global", 1);
    m.insert("us", 23424977);
    m.insert("usa", 23424977);
    m.insert("united states", 23424977);
    m.insert("uk", 23424975);
    m.insert("united kingdom", 23424975);
    m.insert("canada", 23424775);
    m.insert("australia", 23424748);
    m.insert("india", 23424848);
    m.insert("japan", 23424856);
    m.insert("germany", 23424829);
    m.insert("france", 23424819);
    m.insert("brazil", 23424768);
    m.insert("mexico", 23424900);
    m.insert("spain", 23424950);
    m.insert("italy", 23424853);
    m.insert("netherlands", 23424909);
    m.insert("south korea", 23424868);
    m.insert("korea", 23424868);
    m.insert("turkey", 23424969);
    m.insert("indonesia", 23424846);
    m.insert("nigeria", 23424908);
    m.insert("south africa", 23424942);
    m.insert("singapore", 23424948);
    m.insert("new zealand", 23424916);
    m.insert("argentina", 23424747);
    m.insert("colombia", 23424787);
    m.insert("philippines", 23424934);
    m.insert("egypt", 23424802);
    m.insert("israel", 23424852);
    m.insert("ireland", 23424803);
    m.insert("sweden", 23424954);
    m.insert("poland", 23424923);
    m
}

fn woeid_name(woeid: u32) -> &'static str {
    match woeid {
        1 => "Worldwide",
        23424977 => "United States",
        23424975 => "United Kingdom",
        23424775 => "Canada",
        23424748 => "Australia",
        23424848 => "India",
        23424856 => "Japan",
        23424829 => "Germany",
        23424819 => "France",
        23424768 => "Brazil",
        23424900 => "Mexico",
        23424950 => "Spain",
        23424853 => "Italy",
        23424909 => "Netherlands",
        23424868 => "South Korea",
        23424969 => "Turkey",
        23424846 => "Indonesia",
        23424908 => "Nigeria",
        23424942 => "South Africa",
        23424948 => "Singapore",
        23424916 => "New Zealand",
        23424747 => "Argentina",
        23424787 => "Colombia",
        23424934 => "Philippines",
        23424802 => "Egypt",
        23424852 => "Israel",
        23424803 => "Ireland",
        23424954 => "Sweden",
        23424923 => "Poland",
        _ => "Unknown",
    }
}

fn resolve_woeid(input: &str) -> Result<u32> {
    let trimmed = input.trim();
    if let Ok(n) = trimmed.parse::<u32>() {
        return Ok(n);
    }

    let key = trimmed.to_lowercase();
    let map = woeid_map();
    if let Some(&woeid) = map.get(key.as_str()) {
        return Ok(woeid);
    }

    // Fuzzy: check prefix
    for (k, &v) in &map {
        if k.starts_with(&key) {
            return Ok(v);
        }
    }

    anyhow::bail!("Unknown location: \"{}\". Use --locations to list known locations.", input);
}

pub async fn run(args: &TrendsArgs, config: &Config, client: &XClient) -> Result<()> {
    let token = config.require_bearer_token()?;

    // --locations flag
    if args.locations {
        println!("\nKnown locations:\n");
        let map = woeid_map();
        let mut unique: HashMap<u32, String> = HashMap::new();
        for (&name, &woeid) in &map {
            let entry = unique.entry(woeid).or_default();
            if name.len() > entry.len() {
                *entry = name.to_string();
            }
        }
        let mut sorted: Vec<_> = unique.into_iter().collect();
        sorted.sort_by(|a, b| a.1.cmp(&b.1));
        for (woeid, name) in &sorted {
            let display = format!("{}{}", &name[..1].to_uppercase(), &name[1..]);
            println!("  {:<20} WOEID {}", display, woeid);
        }
        return Ok(());
    }

    let location = args
        .location
        .as_ref()
        .map(|v| v.join(" "))
        .unwrap_or_else(|| "worldwide".to_string());

    let woeid = resolve_woeid(&location)?;

    // Check cache
    let cache_key = format!("trends:{}", woeid);
    let cache_ttl = 15 * 60 * 1000u64;

    if !args.no_cache {
        let cached: Option<TrendsResult> =
            crate::cache::get(&config.cache_dir(), &cache_key, "", cache_ttl);
        if let Some(result) = cached {
            if args.json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                print_trends(&result, args.limit);
            }
            return Ok(());
        }
    }

    // Try API endpoint first
    let result = fetch_trends_api(client, token, woeid).await;

    let result = match result {
        Some(r) => {
            costs::track_cost(&config.costs_path(), "trends", "/2/trends", 0);
            r
        }
        None => {
            // Fallback to search-based estimation
            eprintln!("[trends] Falling back to search-based estimation");
            let lang = location_lang(woeid);
            let query = format!("-is:retweet lang:{}", lang);
            let tweets = twitter::search(client, token, &query, 1, "recency", None, None, false).await?;

            costs::track_cost(
                &config.costs_path(),
                "search",
                "/2/tweets/search/recent",
                tweets.len() as u64,
            );

            let mut hashtag_counts: HashMap<String, u64> = HashMap::new();
            for t in &tweets {
                // Extract from hashtags field
                for h in &t.hashtags {
                    let tag = format!("#{}", h.to_lowercase());
                    *hashtag_counts.entry(tag).or_default() += 1;
                }
                // Also extract from text
                for word in t.text.split_whitespace() {
                    if word.starts_with('#') && word.len() > 1 {
                        let tag = word.to_lowercase();
                        *hashtag_counts.entry(tag).or_default() += 1;
                    }
                    if word.starts_with('$') && word.len() > 1 && word[1..].chars().all(|c| c.is_alphabetic()) {
                        let tag = word.to_uppercase();
                        *hashtag_counts.entry(tag).or_default() += 1;
                    }
                }
            }

            let mut sorted: Vec<_> = hashtag_counts
                .into_iter()
                .filter(|(_, count)| *count >= 2)
                .collect();
            sorted.sort_by(|a, b| b.1.cmp(&a.1));

            let trends: Vec<Trend> = sorted
                .into_iter()
                .map(|(name, count)| Trend {
                    url: format!("https://x.com/search?q={}", name),
                    name,
                    tweet_count: Some(count),
                    category: None,
                })
                .collect();

            TrendsResult {
                source: "search_fallback".to_string(),
                location: woeid_name(woeid).to_string(),
                woeid,
                trends,
                fetched_at: chrono::Utc::now().to_rfc3339(),
            }
        }
    };

    // Cache result
    crate::cache::set(&config.cache_dir(), &cache_key, "", &result);

    if args.json {
        println!("{}", serde_json::to_string_pretty(&result)?);
    } else {
        print_trends(&result, args.limit);
    }

    Ok(())
}

async fn fetch_trends_api(client: &XClient, token: &str, woeid: u32) -> Option<TrendsResult> {
    let path = format!("trends/by/woeid/{}", woeid);
    let raw = client.bearer_get(&path, token).await.ok()?;

    // Check if response has data
    let data = raw.data.as_ref()?;
    let arr = data.as_array()?;

    let trends: Vec<Trend> = arr
        .iter()
        .filter_map(|t| {
            let name = t.get("trend_name")?.as_str()?.to_string();
            let tweet_count = t.get("tweet_count").and_then(|v| v.as_u64());
            let category = t.get("category").and_then(|v| v.as_str()).map(|s| s.to_string());
            Some(Trend {
                url: format!("https://x.com/search?q={}", name),
                name,
                tweet_count,
                category,
            })
        })
        .collect();

    if trends.is_empty() {
        return None;
    }

    Some(TrendsResult {
        source: "api".to_string(),
        location: woeid_name(woeid).to_string(),
        woeid,
        trends,
        fetched_at: chrono::Utc::now().to_rfc3339(),
    })
}

fn location_lang(woeid: u32) -> &'static str {
    match woeid {
        1 | 23424977 | 23424975 | 23424775 | 23424748 | 23424916 | 23424803 | 23424948 => "en",
        23424856 => "ja",
        23424829 => "de",
        23424819 => "fr",
        23424768 => "pt",
        23424900 | 23424950 => "es",
        23424853 => "it",
        23424868 => "ko",
        23424969 => "tr",
        23424846 => "id",
        23424923 => "pl",
        23424954 => "sv",
        _ => "en",
    }
}

fn print_trends(result: &TrendsResult, limit: usize) {
    let is_api = result.source == "api";
    let header = if is_api {
        format!("Trending -- {}", result.location)
    } else {
        format!("Trending (estimated from search) -- {}", result.location)
    };

    println!("\n{}\n", header);

    let display: Vec<_> = result.trends.iter().take(limit).collect();

    if display.is_empty() {
        println!("  No trends found.\n");
        return;
    }

    let name_width = display.iter().map(|t| t.name.len()).max().unwrap_or(8).max(8);

    for (i, t) in display.iter().enumerate() {
        let rank = format!("{}.", i + 1);
        let name = format!("{:<width$}", t.name, width = name_width);
        let count_str = match t.tweet_count {
            Some(n) if result.source == "search_fallback" => format!(" -- seen {} times", n),
            Some(n) if n >= 1_000_000 => format!(" -- {:.1}M tweets", n as f64 / 1_000_000.0),
            Some(n) if n >= 1_000 => format!(" -- {:.1}K tweets", n as f64 / 1_000.0),
            Some(n) => format!(" -- {} tweets", n),
            None => String::new(),
        };
        println!("{:>4} {}{}", rank, name, count_str);
    }

    println!();
    if !is_api {
        println!("Note: Using search-based estimation. Official trends API not available on current tier.");
    }
    let fetched_at = &result.fetched_at[..19].replace('T', " ");
    println!("Source: {} | Fetched: {} UTC", result.source, fetched_at);
}
