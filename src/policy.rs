use crate::cli::{Commands, PolicyMode};

pub fn as_str(mode: PolicyMode) -> &'static str {
    match mode {
        PolicyMode::ReadOnly => "read_only",
        PolicyMode::Engagement => "engagement",
        PolicyMode::Moderation => "moderation",
    }
}

fn rank(mode: PolicyMode) -> u8 {
    match mode {
        PolicyMode::ReadOnly => 1,
        PolicyMode::Engagement => 2,
        PolicyMode::Moderation => 3,
    }
}

pub fn command_name(cmd: &Commands) -> &'static str {
    match cmd {
        Commands::Search(_) => "search",
        Commands::Watch(_) => "watch",
        Commands::Stream(_) => "stream",
        Commands::StreamRules(_) => "stream-rules",
        Commands::Diff(_) => "diff",
        Commands::Report(_) => "report",
        Commands::Thread(_) => "thread",
        Commands::Profile(_) => "profile",
        Commands::Tweet(_) => "tweet",
        Commands::Media(_) => "media",
        Commands::Article(_) => "article",
        Commands::Tui(_) => "tui",
        Commands::Bookmarks(_) => "bookmarks",
        Commands::Bookmark(_) => "bookmark",
        Commands::Unbookmark(_) => "unbookmark",
        Commands::Likes(_) => "likes",
        Commands::Like(_) => "like",
        Commands::Unlike(_) => "unlike",
        Commands::Following(_) => "following",
        Commands::Blocks(_) => "blocks",
        Commands::Mutes(_) => "mutes",
        Commands::Follow(_) => "follow",
        Commands::Unfollow(_) => "unfollow",
        Commands::Lists(_) => "lists",
        Commands::Trends(_) => "trends",
        Commands::Analyze(_) => "analyze",
        Commands::Costs(_) => "costs",
        Commands::Health(_) => "health",
        Commands::Capabilities(_) => "capabilities",
        Commands::Watchlist(_) => "watchlist",
        Commands::Auth(_) => "auth",
        Commands::Cache(_) => "cache",
        Commands::XSearch(_) => "x-search",
        Commands::Collections(_) => "collections",
        Commands::Mcp(_) => "mcp",
    }
}

pub fn required_mode(cmd: &Commands) -> PolicyMode {
    match cmd {
        Commands::Blocks(_) | Commands::Mutes(_) => PolicyMode::Moderation,
        Commands::Bookmarks(_)
        | Commands::Bookmark(_)
        | Commands::Unbookmark(_)
        | Commands::Likes(_)
        | Commands::Like(_)
        | Commands::Unlike(_)
        | Commands::Following(_)
        | Commands::Follow(_)
        | Commands::Unfollow(_)
        | Commands::Lists(_)
        | Commands::Diff(_) => PolicyMode::Engagement,
        _ => PolicyMode::ReadOnly,
    }
}

pub fn is_allowed(policy: PolicyMode, required: PolicyMode) -> bool {
    rank(policy) >= rank(required)
}

pub fn emit_policy_denied(cmd: &Commands, policy: PolicyMode, required: PolicyMode) {
    let payload = serde_json::json!({
      "error": {
        "code": "POLICY_DENIED",
        "message": format!("Command '{}' requires '{}' policy mode", command_name(cmd), as_str(required)),
        "command": command_name(cmd),
        "policy_mode": as_str(policy),
        "required_mode": as_str(required),
      }
    });
    eprintln!("{payload}");
}
