mod api;
mod auth;
mod cache;
mod cli;
mod client;
mod commands;
mod config;
mod costs;
mod format;
mod models;
mod sentiment;
mod mcp;

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

    match cli.command {
        Some(Commands::Search(args)) => {
            commands::search::run(&args, &config, &client).await?;
        }
        Some(Commands::Watch(args)) => {
            commands::watch::run(&args, &config, &client).await?;
        }
        Some(Commands::Diff(args)) => {
            commands::diff::run(&args, &config, &client).await?;
        }
        Some(Commands::Report(args)) => {
            commands::report::run(&args, &config, &client).await?;
        }
        Some(Commands::Thread(args)) => {
            commands::thread::run(&args, &config, &client).await?;
        }
        Some(Commands::Profile(args)) => {
            commands::profile::run(&args, &config, &client).await?;
        }
        Some(Commands::Tweet(args)) => {
            commands::tweet::run(&args, &config, &client).await?;
        }
        Some(Commands::Article(args)) => {
            commands::article::run(&args, &config).await?;
        }
        Some(Commands::Bookmarks(args)) => {
            commands::bookmarks::run(&args, &config, &client).await?;
        }
        Some(Commands::Bookmark(args)) => {
            commands::engagement::run_bookmark(&args, &config, &client).await?;
        }
        Some(Commands::Unbookmark(args)) => {
            commands::engagement::run_unbookmark(&args, &config, &client).await?;
        }
        Some(Commands::Likes(args)) => {
            commands::engagement::run_likes(&args, &config, &client).await?;
        }
        Some(Commands::Like(args)) => {
            commands::engagement::run_like(&args, &config, &client).await?;
        }
        Some(Commands::Unlike(args)) => {
            commands::engagement::run_unlike(&args, &config, &client).await?;
        }
        Some(Commands::Following(args)) => {
            commands::engagement::run_following(&args, &config, &client).await?;
        }
        Some(Commands::Trends(args)) => {
            commands::trends::run(&args, &config, &client).await?;
        }
        Some(Commands::Analyze(args)) => {
            commands::analyze::run(&args, &config).await?;
        }
        Some(Commands::Costs(args)) => {
            commands::costs_cmd::run(&args, &config)?;
        }
        Some(Commands::Watchlist(args)) => {
            commands::watchlist::run(&args, &config)?;
        }
        Some(Commands::Auth(args)) => {
            commands::auth_cmd::run(&args, &config, &client).await?;
        }
        Some(Commands::Cache(args)) => {
            commands::cache_cmd::run(&args, &config)?;
        }
        Some(Commands::XSearch(args)) => {
            commands::x_search::run(&args, &config).await?;
        }
        Some(Commands::Collections(args)) => {
            commands::collections::run(&args, &config).await?;
        }
        Some(Commands::Mcp(args)) => {
            mcp::run(args).await?;
        }
        None => {
            // Show help when no command provided
            use clap::CommandFactory;
            Cli::command().print_help()?;
            println!();
        }
    }

    Ok(())
}
