use anyhow::Result;

use crate::auth::oauth;
use crate::cli::AuthArgs;
use crate::client::XClient;
use crate::config::Config;

pub async fn run(args: &AuthArgs, config: &Config, client: &XClient) -> Result<()> {
    let sub = args.subcommand.as_deref().unwrap_or("status");

    match sub {
        "setup" => {
            let client_id = config.require_client_id()?;
            oauth::auth_setup(client, &config.tokens_path(), client_id, args.manual).await?;
        }
        "status" => {
            oauth::auth_status(&config.tokens_path());
        }
        "refresh" => {
            let client_id = config.require_client_id()?;
            let (_, tokens) =
                oauth::get_valid_token(client, &config.tokens_path(), client_id).await?;
            println!("Token refreshed for @{}", tokens.username);
            let expires_in = tokens.expires_at - chrono::Utc::now().timestamp_millis();
            println!("Expires in {} minutes", expires_in / 60_000);
        }
        _ => {
            println!("Usage: xint auth [setup|status|refresh]");
            println!();
            println!("  setup [--manual]    Interactive OAuth setup");
            println!("  status             Show current auth status (default)");
            println!("  refresh            Force token refresh");
        }
    }

    Ok(())
}
