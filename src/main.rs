mod action_result;
mod api;
mod auth;
mod cache;
mod cli;
mod client;
mod commands;
mod config;
mod costs;
mod format;
mod mcp;
mod mcp_dispatcher;
mod models;
mod output_meta;
mod policy;
mod reliability;
mod sentiment;
mod webhook;

use anyhow::Result;
use clap::Parser;

use cli::{Cli, Commands};
use client::XClient;
use config::Config;

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let config = Config::load()?;
    let client = XClient::new()?;

    if let Some(ref cmd) = cli.command {
        let required = policy::required_mode(cmd);
        if !policy::is_allowed(cli.policy, required) {
            policy::emit_policy_denied(cmd, cli.policy, required);
            std::process::exit(2);
        }
    }

    let metric_command = cli
        .command
        .as_ref()
        .map(policy::command_name)
        .map(ToString::to_string);
    let started_at = std::time::Instant::now();

    let result: Result<()> = match cli.command {
        Some(Commands::Search(args)) => commands::search::run(&args, &config, &client).await,
        Some(Commands::Watch(args)) => commands::watch::run(&args, &config, &client).await,
        Some(Commands::Stream(args)) => commands::stream::run_stream(&args, &config, &client).await,
        Some(Commands::StreamRules(args)) => {
            commands::stream::run_stream_rules(&args, &config, &client).await
        }
        Some(Commands::Diff(args)) => commands::diff::run(&args, &config, &client).await,
        Some(Commands::Report(args)) => commands::report::run(&args, &config, &client).await,
        Some(Commands::Thread(args)) => commands::thread::run(&args, &config, &client).await,
        Some(Commands::Profile(args)) => commands::profile::run(&args, &config, &client).await,
        Some(Commands::Tweet(args)) => commands::tweet::run(&args, &config, &client).await,
        Some(Commands::Media(args)) => commands::media::run(&args, &config, &client).await,
        Some(Commands::Article(args)) => commands::article::run(&args, &config).await,
        Some(Commands::Tui(args)) => commands::tui::run(&args, cli.policy).await,
        Some(Commands::Bookmarks(args)) => commands::bookmarks::run(&args, &config, &client).await,
        Some(Commands::Bookmark(args)) => {
            commands::engagement::run_bookmark(&args, &config, &client).await
        }
        Some(Commands::Unbookmark(args)) => {
            commands::engagement::run_unbookmark(&args, &config, &client).await
        }
        Some(Commands::Likes(args)) => {
            commands::engagement::run_likes(&args, &config, &client).await
        }
        Some(Commands::Like(args)) => commands::engagement::run_like(&args, &config, &client).await,
        Some(Commands::Unlike(args)) => {
            commands::engagement::run_unlike(&args, &config, &client).await
        }
        Some(Commands::Following(args)) => {
            commands::engagement::run_following(&args, &config, &client).await
        }
        Some(Commands::Blocks(args)) => {
            commands::moderation::run_blocks(&args, &config, &client).await
        }
        Some(Commands::Mutes(args)) => {
            commands::moderation::run_mutes(&args, &config, &client).await
        }
        Some(Commands::Follow(args)) => {
            commands::engagement::run_follow(&args, &config, &client).await
        }
        Some(Commands::Unfollow(args)) => {
            commands::engagement::run_unfollow(&args, &config, &client).await
        }
        Some(Commands::Lists(args)) => commands::lists::run(&args, &config, &client).await,
        Some(Commands::Trends(args)) => commands::trends::run(&args, &config, &client).await,
        Some(Commands::Analyze(args)) => commands::analyze::run(&args, &config).await,
        Some(Commands::Costs(args)) => commands::costs_cmd::run(&args, &config),
        Some(Commands::Health(args)) => commands::health::run(&args, &config, &client).await,
        Some(Commands::Capabilities(args)) => commands::capabilities::run(&args),
        Some(Commands::Watchlist(args)) => commands::watchlist::run(&args, &config),
        Some(Commands::Auth(args)) => commands::auth_cmd::run(&args, &config, &client).await,
        Some(Commands::Cache(args)) => commands::cache_cmd::run(&args, &config),
        Some(Commands::XSearch(args)) => commands::x_search::run(&args, &config).await,
        Some(Commands::Collections(args)) => commands::collections::run(&args, &config).await,
        Some(Commands::Mcp(args)) => mcp::run(args, &config, cli.policy).await,
        None => {
            // Show help when no command provided
            use clap::CommandFactory;
            Cli::command().print_help()?;
            println!();
            Ok(())
        }
    };

    if let Some(command) = metric_command {
        let fallback = reliability::consume_command_fallback(&command);
        let success = result.is_ok();
        reliability::record_command_result(
            &config.reliability_path(),
            &command,
            success,
            started_at.elapsed().as_millis(),
            reliability::ReliabilityMode::Cli,
            fallback,
        );
    }

    result?;

    Ok(())
}
