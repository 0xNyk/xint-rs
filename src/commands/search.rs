use anyhow::Result;
use std::fs;

use crate::api::twitter;
use crate::cli::SearchArgs;
use crate::client::XClient;
use crate::config::Config;
use crate::costs;
use crate::format;
use crate::output_meta;
use crate::sentiment;

pub async fn run(args: &SearchArgs, config: &Config, client: &XClient) -> Result<()> {
    let started_at = std::time::Instant::now();
    let token = config.require_bearer_token()?;
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
        let slug = query
            .chars()
            .filter(|c| c.is_alphanumeric() || *c == ' ')
            .collect::<String>()
            .replace(' ', "-")
            .to_lowercase();
        let slug = &slug[..slug.len().min(40)];
        let path = exports_dir.join(format!("search-{slug}-{date}.md"));
        let md = format::format_research_markdown(&query, &tweets, &[&query]);
        fs::write(&path, &md)?;
        eprintln!("\nSaved to {}", path.display());
    }

    Ok(())
}
