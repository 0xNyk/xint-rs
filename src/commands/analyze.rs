use anyhow::Result;
use std::fs;
use std::io::Read;

use crate::api::grok;
use crate::cli::AnalyzeArgs;
use crate::config::Config;
use crate::models::*;

pub async fn run(args: &AnalyzeArgs, config: &Config) -> Result<()> {
    let api_key = config.require_xai_key()?;
    let http = reqwest::Client::new();

    let opts = GrokOpts {
        model: args.model.clone(),
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
        grok::analyze_tweets(&http, api_key, &tweets, prompt.as_deref(), &opts).await?
    } else if let Some(ref tweet_file) = args.tweets {
        let raw = fs::read_to_string(tweet_file)?;
        let tweets = parse_tweets_input(&raw)?;
        let prompt = if args.query.is_empty() {
            None
        } else {
            Some(args.query.join(" "))
        };
        grok::analyze_tweets(&http, api_key, &tweets, prompt.as_deref(), &opts).await?
    } else if !args.query.is_empty() {
        let query = args.query.join(" ");
        let messages = vec![
            GrokMessage {
                role: "system".to_string(),
                content: args
                    .system
                    .clone()
                    .unwrap_or_else(|| {
                        "You are a social media analyst. Provide concise, actionable insights."
                            .to_string()
                    }),
            },
            GrokMessage {
                role: "user".to_string(),
                content: query,
            },
        ];
        grok::grok_chat(&http, api_key, &messages, &opts).await?
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
