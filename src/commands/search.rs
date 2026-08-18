use anyhow::Result;
use std::fs;

use crate::api::hermes_tweet::{HermesTweetClient, TwitterBackend};
use crate::api::twitter;
use crate::cli::SearchArgs;
use crate::client::XClient;
use crate::config::Config;
use crate::costs;
use crate::format;
use crate::output_meta;
use crate::sentiment;

fn export_slug(query: &str) -> String {
    query
        .chars()
        .filter(|character| character.is_alphanumeric() || *character == ' ')
        .collect::<String>()
        .replace(' ', "-")
        .to_lowercase()
        .chars()
        .take(40)
        .collect()
}

pub async fn run(args: &SearchArgs, config: &Config, client: &XClient) -> Result<()> {
    let started_at = std::time::Instant::now();
    let mut query = args.query.join(" ");

    if query.is_empty() {
        println!("Usage: xint search <query>");
        return Ok(());
    }

    // --from shorthand
    if let Some(ref from) = args.from {
        let user = from.trim_start_matches('@');
        query = format!("from:{user} {query}").trim().to_string();
    }

    // --no-replies / --no-retweets modifiers
    if args.no_replies {
        query.push_str(" -is:reply");
    }
    if args.no_retweets {
        query.push_str(" -is:retweet");
    }

    if config.twitter_backend() == TwitterBackend::HermesTweet {
        return run_hermes_tweet_search(args, config, &query, started_at).await;
    }

    // Dry-run preview — print estimate and exit before any API call.
    if args.dry_run {
        let pages_est = args.pages.clamp(1, 5);
        let per_page = if args.full { 500u64 } else { 100u64 };
        let units = pages_est as u64 * per_page;
        let op = if args.full { "search_archive" } else { "search" };
        let endpoint = if args.full {
            "/2/tweets/search/all"
        } else {
            "/2/tweets/search/recent"
        };
        let cost = crate::dryrun::estimate_cost(op, units, 1);
        let notes: Vec<&str> = if args.full {
            vec!["Full-archive search: 2x cost; requires --confirm to actually run."]
        } else {
            vec![]
        };
        crate::dryrun::preview_and_exit(&crate::dryrun::DryRunEstimate {
            command: &format!("search \"{query}\""),
            endpoint,
            units,
            unit_label: "tweets",
            cost_usd: cost,
            cache_predicted_hit: None,
            cache_ttl_minutes: Some(if args.quick { 60 } else { 15 }),
            notes: &notes,
        });
    }

    let token = config.require_bearer_token()?;

    // Archive search confirmation gate — full-archive is 2x cost and easy
    // to invoke accidentally. Require explicit --confirm so users opt in
    // knowing the price.
    if args.full && !args.confirm {
        let est_pages = args.pages.clamp(1, 5);
        let est_cost = est_pages as f64 * 500.0 * 0.01;
        eprintln!(
            "\n⚠  Full-archive search is 2x the cost of recent search.\n   Estimated max cost: ~${est_cost:.2} ({est_pages} page(s) × 500 tweets × $0.01).\n   Re-run with --confirm to proceed, or drop --full to use recent search instead.\n"
        );
        std::process::exit(2);
    }

    // Quick mode overrides
    let (pages, limit, min_likes, cache_ttl) = if args.quick {
        (1u32, args.limit.min(10), 5u64, 60 * 60 * 1000u64) // 1hr cache
    } else {
        (
            args.pages.min(5),
            args.limit,
            args.min_likes,
            15 * 60 * 1000u64,
        )
    };

    // Quality mode
    let min_likes = if args.quality && min_likes < 10 {
        10
    } else {
        min_likes
    };

    // Check cache
    let cache_key = format!("search:{query}");
    let cache_params = format!("p={}&s={}", pages, args.sort);
    let cached: Option<Vec<crate::models::Tweet>> =
        crate::cache::get(&config.cache_dir(), &cache_key, &cache_params, cache_ttl);

    let mut cache_hit = false;
    let mut tweets = if let Some(cached) = cached {
        cache_hit = true;
        eprintln!("(cached \u{2014} {} tweets)", cached.len());
        cached
    } else {
        let sort_order = match args.sort.as_str() {
            "recent" | "recency" => "recency",
            _ => "relevancy",
        };

        let spinner = crate::spinner::Spinner::new(&format!("Searching \"{query}\"..."));
        let tweets = twitter::search(
            client,
            token,
            &query,
            pages,
            sort_order,
            args.since.as_deref(),
            args.until.as_deref(),
            args.full,
        )
        .await;
        match &tweets {
            Ok(t) => spinner.done(&format!("Found {} tweets", t.len())),
            Err(_) => spinner.fail("Search failed"),
        }
        let tweets = tweets?;

        // Track cost
        costs::track_cost(
            &config.costs_path(),
            if args.full {
                "search_archive"
            } else {
                "search"
            },
            "/2/tweets/search/recent",
            tweets.len() as u64,
        );

        // Cache results
        crate::cache::set(&config.cache_dir(), &cache_key, &cache_params, &tweets);

        tweets
    };

    // Post-processing
    tweets = twitter::dedupe(tweets);
    tweets = twitter::filter_engagement(tweets, min_likes, args.min_impressions);

    match args.sort.as_str() {
        "recent" | "recency" => {} // already sorted by recency from API
        other => twitter::sort_by(&mut tweets, other),
    }

    // Sentiment analysis
    if args.sentiment {
        if let Ok(api_key) = config.require_xai_key() {
            let http = reqwest::Client::new();
            eprintln!("Running sentiment analysis...");
            match sentiment::analyze_sentiment(
                &http,
                api_key,
                &tweets,
                None,
                Some(&config.costs_path()),
            )
            .await
            {
                Ok(sentiments) => {
                    let stats = sentiment::compute_stats(&sentiments);
                    eprint!("{}", sentiment::format_stats(&stats, tweets.len()));
                }
                Err(e) => eprintln!("[sentiment] Failed: {e}"),
            }
        } else {
            eprintln!("[sentiment] XAI_API_KEY not set, skipping sentiment analysis");
        }
    }

    // Output
    let shown: Vec<_> = tweets.iter().take(limit).cloned().collect();
    let endpoint = if args.full {
        "/2/tweets/search/all"
    } else {
        "/2/tweets/search/recent"
    };
    let estimated_cost_usd = if cache_hit {
        0.0
    } else if args.full {
        tweets.len() as f64 * 0.01
    } else {
        tweets.len() as f64 * 0.005
    };
    let meta = output_meta::build_meta(
        "x_api_v2",
        started_at,
        cache_hit,
        1.0,
        endpoint,
        estimated_cost_usd,
        &config.costs_path(),
    );

    if args.json {
        output_meta::print_json_with_meta(&meta, &shown)?;
    } else if args.jsonl {
        output_meta::print_jsonl_with_meta(&meta, "tweet", &shown)?;
    } else if args.csv {
        let output = format::format_csv(&tweets[..tweets.len().min(limit)]);
        println!("{output}");
    } else if args.markdown {
        let output =
            format::format_research_markdown(&query, &tweets[..tweets.len().min(limit)], &[&query]);
        println!("{output}");
    } else {
        let output = format::format_results_terminal(&tweets, Some(&query), limit);
        println!("{output}");
    }

    // Save to exports
    if args.save {
        let exports_dir = config.exports_dir();
        fs::create_dir_all(&exports_dir)?;
        let date = chrono::Utc::now().format("%Y-%m-%d").to_string();
        let slug = export_slug(&query);
        let path = exports_dir.join(format!("search-{slug}-{date}.md"));
        let md = format::format_research_markdown(&query, &tweets, &[&query]);
        fs::write(&path, &md)?;
        eprintln!("\nSaved to {}", path.display());
    }

    Ok(())
}

async fn run_hermes_tweet_search(
    args: &SearchArgs,
    config: &Config,
    query: &str,
    started_at: std::time::Instant,
) -> Result<()> {
    if args.full {
        anyhow::bail!("Hermes Tweet backend supports recent public search only; remove --full");
    }
    let limit = if args.quick {
        args.limit.min(10)
    } else {
        args.limit
    };
    let fetch_limit = limit.clamp(1, 100);

    if args.dry_run {
        crate::dryrun::preview_and_exit(&crate::dryrun::DryRunEstimate {
            command: &format!("search \"{query}\""),
            endpoint: "/api/v1/x/tweets/search",
            units: fetch_limit as u64,
            unit_label: "tweets",
            cost_usd: 0.0,
            cache_predicted_hit: None,
            cache_ttl_minutes: None,
            notes: &["Hermes Tweet/Xquik is an optional read-only search backend."],
        });
    }

    let api_key = config.require_hermes_tweet_key()?;
    let backend = HermesTweetClient::new(config.hermes_tweet_api_base.as_deref(), api_key)?;

    let spinner =
        crate::spinner::Spinner::new(&format!("Searching \"{query}\" with Hermes Tweet..."));
    let tweets = backend.search(query, fetch_limit).await;
    match &tweets {
        Ok(t) => spinner.done(&format!("Found {} tweets", t.len())),
        Err(_) => spinner.fail("Hermes Tweet search failed"),
    }
    let mut tweets = tweets?;
    tweets = twitter::dedupe(tweets);
    tweets = twitter::filter_engagement(tweets, args.min_likes, args.min_impressions);
    match args.sort.as_str() {
        "recent" | "recency" => {}
        other => twitter::sort_by(&mut tweets, other),
    }

    if args.sentiment {
        if let Ok(api_key) = config.require_xai_key() {
            let http = reqwest::Client::new();
            eprintln!("Running sentiment analysis...");
            match sentiment::analyze_sentiment(
                &http,
                api_key,
                &tweets,
                None,
                Some(&config.costs_path()),
            )
            .await
            {
                Ok(sentiments) => {
                    let stats = sentiment::compute_stats(&sentiments);
                    eprint!("{}", sentiment::format_stats(&stats, tweets.len()));
                }
                Err(e) => eprintln!("[sentiment] Failed: {e}"),
            }
        } else {
            eprintln!("[sentiment] XAI_API_KEY not set, skipping sentiment analysis");
        }
    }

    let shown: Vec<_> = tweets.iter().take(limit).cloned().collect();
    let meta = output_meta::build_meta(
        "hermes_tweet",
        started_at,
        false,
        1.0,
        "/api/v1/x/tweets/search",
        0.0,
        &config.costs_path(),
    );

    if args.json {
        output_meta::print_json_with_meta(&meta, &shown)?;
    } else if args.jsonl {
        output_meta::print_jsonl_with_meta(&meta, "tweet", &shown)?;
    } else if args.csv {
        let output = format::format_csv(&tweets[..tweets.len().min(limit)]);
        println!("{output}");
    } else if args.markdown {
        let output =
            format::format_research_markdown(query, &tweets[..tweets.len().min(limit)], &[query]);
        println!("{output}");
    } else {
        let output = format::format_results_terminal(&tweets, Some(query), limit);
        println!("{output}");
    }

    if args.save {
        let exports_dir = config.exports_dir();
        fs::create_dir_all(&exports_dir)?;
        let date = chrono::Utc::now().format("%Y-%m-%d").to_string();
        let slug = export_slug(query);
        let path = exports_dir.join(format!("search-{slug}-{date}.md"));
        let md = format::format_research_markdown(query, &tweets, &[query]);
        fs::write(&path, &md)?;
        eprintln!("\nSaved to {}", path.display());
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::export_slug;

    #[test]
    fn export_slug_normalizes_spaces_and_case() {
        assert_eq!(export_slug("X Search Results"), "x-search-results");
    }

    #[test]
    fn export_slug_truncates_at_a_unicode_character_boundary() {
        let query = format!("{}éclair", "a".repeat(39));
        let slug = export_slug(&query);

        assert_eq!(slug.chars().count(), 40);
        assert!(slug.ends_with('é'));
    }
}
