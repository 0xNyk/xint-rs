use anyhow::{bail, Result};
use rand::RngCore;
use sha2::{Digest, Sha256};
use std::fs;
use std::io::{self, BufRead, Write as IoWrite};
use std::path::Path;

use crate::client::XClient;
use crate::models::OAuthTokens;

const AUTHORIZE_URL: &str = "https://x.com/i/oauth2/authorize";
const TOKEN_URL: &str = "https://api.x.com/2/oauth2/token";
const REDIRECT_URI: &str = "http://127.0.0.1:3333/callback";
const SCOPES: &str = "bookmark.read bookmark.write like.read like.write follows.read follows.write list.read list.write block.read block.write mute.read mute.write tweet.read tweet.write users.read offline.access";
const EXPIRY_BUFFER_MS: i64 = 60_000;

// ---------------------------------------------------------------------------
// PKCE helpers
// ---------------------------------------------------------------------------

fn base64url(bytes: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

pub fn generate_code_verifier() -> String {
    let mut bytes = [0u8; 32];
    if getrandom::fill(&mut bytes).is_err() {
        rand::thread_rng().fill_bytes(&mut bytes);
    }
    base64url(&bytes)
}

pub fn generate_code_challenge(verifier: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(verifier.as_bytes());
    let hash = hasher.finalize();
    base64url(&hash)
}

pub fn generate_state() -> String {
    let mut bytes = [0u8; 16];
    if getrandom::fill(&mut bytes).is_err() {
        rand::thread_rng().fill_bytes(&mut bytes);
    }
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

// ---------------------------------------------------------------------------
// Token storage
// ---------------------------------------------------------------------------

pub fn load_tokens(tokens_path: &Path) -> Option<OAuthTokens> {
    if !tokens_path.exists() {
        return None;
    }
    let content = fs::read_to_string(tokens_path).ok()?;
    serde_json::from_str(&content).ok()
}

fn save_tokens(tokens_path: &Path, tokens: &OAuthTokens) -> Result<()> {
    if let Some(parent) = tokens_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = tokens_path.with_extension("json.tmp");
    let json = serde_json::to_string_pretty(tokens)?;
    fs::write(&tmp, &json)?;

    // Set file permissions to 600 on unix
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&tmp, fs::Permissions::from_mode(0o600))?;
    }

    fs::rename(&tmp, tokens_path)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Token exchange / refresh
// ---------------------------------------------------------------------------

async fn exchange_code(
    client: &XClient,
    code: &str,
    code_verifier: &str,
    client_id: &str,
) -> Result<serde_json::Value> {
    client
        .post_form(
            TOKEN_URL,
            &[
                ("grant_type", "authorization_code"),
                ("code", code),
                ("redirect_uri", REDIRECT_URI),
                ("client_id", client_id),
                ("code_verifier", code_verifier),
            ],
        )
        .await
}

async fn fetch_user_me(client: &XClient, access_token: &str) -> Result<(String, String)> {
    let raw = client.oauth_get("users/me", access_token).await?;
    let data = &raw.data;
    let id = data
        .as_ref()
        .and_then(|d| d.get("id"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("Failed to get user ID"))?
        .to_string();
    let username = data
        .as_ref()
        .and_then(|d| d.get("username"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("Failed to get username"))?
        .to_string();
    Ok((id, username))
}

/// Refresh an existing OAuth token.
pub async fn refresh_tokens(
    client: &XClient,
    tokens_path: &Path,
    client_id: &str,
    tokens: &OAuthTokens,
) -> Result<OAuthTokens> {
    let data = client
        .post_form(
            TOKEN_URL,
            &[
                ("grant_type", "refresh_token"),
                ("refresh_token", &tokens.refresh_token),
                ("client_id", client_id),
            ],
        )
        .await?;

    let now = chrono::Utc::now();
    let expires_in = data
        .get("expires_in")
        .and_then(|v| v.as_i64())
        .unwrap_or(7200);

    let updated = OAuthTokens {
        access_token: data
            .get("access_token")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        refresh_token: data
            .get("refresh_token")
            .and_then(|v| v.as_str())
            .unwrap_or(&tokens.refresh_token)
            .to_string(),
        expires_at: now.timestamp_millis() + expires_in * 1000,
        user_id: tokens.user_id.clone(),
        username: tokens.username.clone(),
        scope: data
            .get("scope")
            .and_then(|v| v.as_str())
            .unwrap_or(&tokens.scope)
            .to_string(),
        created_at: tokens.created_at.clone(),
        refreshed_at: now.to_rfc3339(),
    };

    save_tokens(tokens_path, &updated)?;
    Ok(updated)
}

/// Get a valid access token, auto-refreshing if expired.
pub async fn get_valid_token(
    client: &XClient,
    tokens_path: &Path,
    client_id: &str,
) -> Result<(String, OAuthTokens)> {
    let tokens = load_tokens(tokens_path)
        .ok_or_else(|| anyhow::anyhow!("No OAuth tokens found. Run 'auth setup' first."))?;

    let now = chrono::Utc::now().timestamp_millis();

    if now >= tokens.expires_at - EXPIRY_BUFFER_MS {
        eprintln!("OAuth token expired, refreshing...");
        let refreshed = refresh_tokens(client, tokens_path, client_id, &tokens).await?;
        eprintln!("Token refreshed for @{}", refreshed.username);
        let token = refreshed.access_token.clone();
        Ok((token, refreshed))
    } else {
        let token = tokens.access_token.clone();
        Ok((token, tokens))
    }
}

// ---------------------------------------------------------------------------
// Auth setup
// ---------------------------------------------------------------------------

fn build_authorize_url(client_id: &str, code_challenge: &str, state: &str) -> String {
    format!(
        "{}?response_type=code&client_id={}&redirect_uri={}&scope={}&state={}&code_challenge={}&code_challenge_method=S256",
        AUTHORIZE_URL,
        client_id,
        urlencoding_encode(REDIRECT_URI),
        urlencoding_encode(SCOPES),
        state,
        code_challenge
    )
}

fn urlencoding_encode(s: &str) -> String {
    let mut result = String::new();
    for c in s.chars() {
        match c {
            'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '_' | '.' | '~' => result.push(c),
            ' ' => result.push_str("%20"),
            _ => {
                for byte in c.to_string().as_bytes() {
                    result.push_str(&format!("%{byte:02X}"));
                }
            }
        }
    }
    result
}

/// Interactive auth setup flow.
pub async fn auth_setup(
    client: &XClient,
    tokens_path: &Path,
    client_id: &str,
    manual: bool,
) -> Result<()> {
    let code_verifier = generate_code_verifier();
    let code_challenge = generate_code_challenge(&code_verifier);
    let state = generate_state();
    let authorize_url = build_authorize_url(client_id, &code_challenge, &state);

    eprintln!("\n=== X API OAuth 2.0 Setup (PKCE) ===\n");
    eprintln!("1. Open this URL in your browser:\n");
    eprintln!("{authorize_url}");
    eprintln!("\n2. Authorize the app, then:");

    let code = if manual {
        eprintln!("   Paste the full redirect URL here:\n");
        eprint!("Redirect URL> ");
        io::stderr().flush()?;

        let mut input = String::new();
        io::stdin().lock().read_line(&mut input)?;
        let input = input.trim();

        // Parse the URL to extract code and state
        let (returned_state, code) = parse_callback_url(input)?;
        if returned_state != state {
            bail!("State mismatch — possible CSRF attack. Aborting.");
        }
        code
    } else {
        eprintln!("   The browser will redirect to localhost:3333\n");
        wait_for_callback(&state).await?
    };

    eprintln!("\nExchanging authorization code for tokens...");
    let token_data = exchange_code(client, &code, &code_verifier, client_id).await?;

    let access_token = token_data
        .get("access_token")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("No access_token in response"))?;

    eprintln!("Fetching user info...");
    let (user_id, username) = fetch_user_me(client, access_token).await?;

    let now = chrono::Utc::now();
    let expires_in = token_data
        .get("expires_in")
        .and_then(|v| v.as_i64())
        .unwrap_or(7200);

    let tokens = OAuthTokens {
        access_token: access_token.to_string(),
        refresh_token: token_data
            .get("refresh_token")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        expires_at: now.timestamp_millis() + expires_in * 1000,
        user_id: user_id.clone(),
        username: username.clone(),
        scope: token_data
            .get("scope")
            .and_then(|v| v.as_str())
            .unwrap_or(SCOPES)
            .to_string(),
        created_at: now.to_rfc3339(),
        refreshed_at: now.to_rfc3339(),
    };

    save_tokens(tokens_path, &tokens)?;

    eprintln!("\nAuthenticated as @{username} (ID: {user_id})");
    eprintln!("Token expires in {} minutes", expires_in / 60);
    eprintln!("Refresh token valid for ~6 months");
    eprintln!("Tokens saved to {}", tokens_path.display());

    Ok(())
}

fn parse_callback_url(url_str: &str) -> Result<(String, String)> {
    // Simple URL parameter extraction
    let query = if let Some(pos) = url_str.find('?') {
        &url_str[pos + 1..]
    } else {
        bail!("Invalid redirect URL: no query parameters");
    };

    let mut state = String::new();
    let mut code = String::new();
    let mut error = String::new();

    for pair in query.split('&') {
        let mut parts = pair.splitn(2, '=');
        let key = parts.next().unwrap_or("");
        let value = parts.next().unwrap_or("");
        match key {
            "state" => state = value.to_string(),
            "code" => code = value.to_string(),
            "error" => error = value.to_string(),
            _ => {}
        }
    }

    if !error.is_empty() {
        bail!("Authorization failed: {error}");
    }
    if code.is_empty() {
        bail!("No authorization code in redirect URL");
    }

    Ok((state, code))
}

/// Wait for the OAuth callback on localhost:3333.
async fn wait_for_callback(expected_state: &str) -> Result<String> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    let listener = TcpListener::bind("127.0.0.1:3333").await?;
    eprintln!("Waiting for callback on http://127.0.0.1:3333/callback ...");
    eprintln!("(timeout in 5 minutes — use Ctrl+C or --manual to abort)\n");

    let timeout = tokio::time::timeout(std::time::Duration::from_secs(300), async {
        loop {
            let (mut socket, _) = listener.accept().await?;
            let mut buf = vec![0u8; 4096];
            let n = socket.read(&mut buf).await?;
            let request = String::from_utf8_lossy(&buf[..n]);

            // Extract path from GET request
            let first_line = request.lines().next().unwrap_or("");
            let path = first_line.split_whitespace().nth(1).unwrap_or("/");

            if !path.starts_with("/callback") {
                let response =
                    "HTTP/1.1 404 Not Found\r\nContent-Type: text/html\r\n\r\n<h2>Not Found</h2>";
                let _ = socket.write_all(response.as_bytes()).await;
                continue;
            }

            let query = path.split('?').nth(1).unwrap_or("");
            let mut state = String::new();
            let mut code = String::new();
            let mut error = String::new();

            for pair in query.split('&') {
                let mut parts = pair.splitn(2, '=');
                let key = parts.next().unwrap_or("");
                let value = parts.next().unwrap_or("");
                match key {
                    "state" => state = value.to_string(),
                    "code" => code = value.to_string(),
                    "error" => error = value.to_string(),
                    _ => {}
                }
            }

            if !error.is_empty() {
                let html = format!("<html><body><h2>Authorization Failed</h2><p>{error}</p><p>You can close this tab.</p></body></html>");
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\n\r\n{}",
                    html.len(),
                    html
                );
                let _ = socket.write_all(response.as_bytes()).await;
                return Err(anyhow::anyhow!("Authorization denied: {error}"));
            }

            if state != expected_state {
                let html = "<html><body><h2>Error: State Mismatch</h2><p>Possible CSRF attack. Please try again.</p></body></html>";
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\n\r\n{}",
                    html.len(),
                    html
                );
                let _ = socket.write_all(response.as_bytes()).await;
                return Err(anyhow::anyhow!("State mismatch — possible CSRF attack."));
            }

            if code.is_empty() {
                let html = "<html><body><h2>Error</h2><p>No authorization code received.</p></body></html>";
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\n\r\n{}",
                    html.len(),
                    html
                );
                let _ = socket.write_all(response.as_bytes()).await;
                return Err(anyhow::anyhow!("No authorization code received."));
            }

            let html = "<html><body><h2>Authorization Successful!</h2><p>You can close this tab and return to the terminal.</p></body></html>";
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\n\r\n{}",
                html.len(),
                html
            );
            let _ = socket.write_all(response.as_bytes()).await;

            return Ok::<String, anyhow::Error>(code);
        }
    });

    match timeout.await {
        Ok(result) => result,
        Err(_) => bail!("Callback timeout (5 minutes). Try again with --manual flag."),
    }
}

/// Print auth status.
pub fn auth_status(tokens_path: &Path) {
    let tokens = match load_tokens(tokens_path) {
        Some(t) => t,
        None => {
            println!("No OAuth tokens found.");
            println!("Run: xint auth setup [--manual]");
            return;
        }
    };

    let now = chrono::Utc::now().timestamp_millis();
    let expires_in = tokens.expires_at - now;
    let expires_str = if expires_in > 0 {
        format!("{} minutes", expires_in / 60_000)
    } else {
        "EXPIRED".to_string()
    };

    println!(
        "Authenticated as @{} (ID: {})",
        tokens.username, tokens.user_id
    );
    println!("   Scopes: {}", tokens.scope);
    println!("   Access token: {expires_str}");
    println!("   Created: {}", tokens.created_at);
    println!("   Last refresh: {}", tokens.refreshed_at);
}
