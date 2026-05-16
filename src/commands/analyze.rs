use anyhow::Result;
use std::fs;
use std::io::Read;
use std::str::FromStr;

use crate::api::grok::{self, Budget};
use crate::cli::AnalyzeArgs;
use crate::config::Config;
use crate::models::*;

pub async fn run(args: &AnalyzeArgs, config: &Config) -> Result<()> {
    // Premium status from XINT_X_PREMIUM env var (user-set; no API call).
    let premium_status = std::env::var("XINT_X_PREMIUM").ok();

    // Pre-flight: if no XAI_API_KEY, print credit guide and exit 0 so the
    // caller (often an AI agent) can read the onboarding text programmatically.
    let api_key = match config.try_xai_key() {
        Some(k) => k,
        None => {
            super::credits::print_credit_guide(premium_status.as_deref());
            return Ok(());
        }
    };

    // Premium-routing tip — opt-in via XINT_PREMIUM_TIPS=1 to avoid noise.
    if std::env::var("XINT_PREMIUM_TIPS").as_deref() == Ok("1") {
        let query = args.query.join(" ");
        if let Some(tip) = super::credits::premium_chat_routing_tip(
            premium_status.as_deref(),
            args.pipe,
            args.tweets.as_deref(),
            None, // image flag not wired to Rust yet
            &query,
        ) {
            eprintln!("{tip}\n");
        }
    }

    let http = reqwest::Client::new();

    let budget = args
        .budget
        .as_deref()
        .map(Budget::from_str)
        .transpose()
        .map_err(|e| anyhow::anyhow!(e))?;
    let has_image = false; // image flag is TS-side only for now; PR3 mirrors it
    let model = grok::resolve_model(args.model.as_deref(), budget, has_image);

    let opts = GrokOpts {
        model,
        ..Default::default()
    };

    let response = if args.pipe {
        // Read tweets from stdin
        let mut input = String::new();
        std::io::stdin().read_to_string(&mut input)?;
        let input = input.trim();
        if input.is_empty() {
            anyhow::bail!("No input received on stdin. Pipe tweet JSON or use --tweets <file>.");
        }
        let tweets = parse_tweets_input(input)?;
        let prompt = if args.query.is_empty() {
            None
        } else {
            Some(args.query.join(" "))
        };
        grok::analyze_tweets_tracked(
            &http,
            api_key,
            &tweets,
            prompt.as_deref(),
            &opts,
            Some(&config.costs_path()),
        )
        .await?
    } else if let Some(ref tweet_file) = args.tweets {
        let raw = fs::read_to_string(tweet_file)?;
        let tweets = parse_tweets_input(&raw)?;
        let prompt = if args.query.is_empty() {
            None
        } else {
            Some(args.query.join(" "))
        };
        grok::analyze_tweets_tracked(
            &http,
            api_key,
            &tweets,
            prompt.as_deref(),
            &opts,
            Some(&config.costs_path()),
        )
        .await?
    } else if !args.query.is_empty() {
        let query = args.query.join(" ");
        let messages = vec![
            GrokMessage {
                role: "system".to_string(),
                content: "You are a social media analyst. Provide concise, actionable insights."
                    .to_string(),
            },
            GrokMessage {
                role: "user".to_string(),
                content: query,
            },
        ];
        grok::grok_chat_tracked(&http, api_key, &messages, &opts, Some(&config.costs_path()))
            .await?
    } else {
        println!("Usage: xint analyze <query>");
        println!("       xint analyze --tweets <file>");
        println!("       xint analyze --pipe");
        return Ok(());
    };

    // Format output
    let cost = grok::estimate_cost(
        &response.model,
        response.usage.prompt_tokens,
        response.usage.completion_tokens,
    );

    println!("\nGrok Analysis ({})\n", response.model);
    println!("{}", response.content);
    println!("\n---");
    println!(
        "Tokens: {} prompt + {} completion = {} total",
        response.usage.prompt_tokens, response.usage.completion_tokens, response.usage.total_tokens
    );
    println!("Model: {} | Est. cost: {}", response.model, cost);

    Ok(())
}

fn parse_tweets_input(raw: &str) -> Result<Vec<Tweet>> {
    let parsed: serde_json::Value = serde_json::from_str(raw)?;

    let arr = if let Some(arr) = parsed.as_array() {
        arr.clone()
    } else if let Some(arr) = parsed.get("tweets").and_then(|v| v.as_array()) {
        arr.clone()
    } else {
        anyhow::bail!("Expected a JSON array of tweets or {{ tweets: [...] }}");
    };

    if arr.is_empty() {
        anyhow::bail!("No tweets found in input");
    }

    let tweets: Vec<Tweet> = arr
        .into_iter()
        .filter_map(|v| serde_json::from_value(v).ok())
        .collect();

    if tweets.is_empty() {
        anyhow::bail!("Failed to parse tweets from input");
    }

    Ok(tweets)
}
