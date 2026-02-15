use anyhow::Result;
use std::path::PathBuf;

/// Resolved configuration from env vars and .env file.
pub struct Config {
    pub bearer_token: Option<String>,
    pub client_id: Option<String>,
    pub xai_api_key: Option<String>,
    pub xai_management_api_key: Option<String>,
    pub data_dir: PathBuf,
}

impl Config {
    /// Load configuration from environment and optional .env file.
    pub fn load() -> Result<Self> {
        // Try loading .env from current dir, then from binary dir
        let _ = dotenvy::dotenv();

        // Also try .env next to the binary
        if let Ok(exe) = std::env::current_exe() {
            if let Some(parent) = exe.parent() {
                let env_path = parent.join(".env");
                if env_path.exists() {
                    let _ = dotenvy::from_path(&env_path);
                }
            }
        }

        let bearer_token = std::env::var("X_BEARER_TOKEN").ok();
        let client_id = std::env::var("X_CLIENT_ID").ok();
        let xai_api_key = std::env::var("XAI_API_KEY").ok();
        let xai_management_api_key = std::env::var("XAI_MANAGEMENT_API_KEY").ok();

        // Data dir: ./data/ relative to binary, or current dir
        let data_dir = resolve_data_dir();

        Ok(Self {
            bearer_token,
            client_id,
            xai_api_key,
            xai_management_api_key,
            data_dir,
        })
    }

    pub fn require_bearer_token(&self) -> Result<&str> {
        self.bearer_token
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("X_BEARER_TOKEN not found. Set it in your environment or in .env"))
    }

    pub fn require_client_id(&self) -> Result<&str> {
        self.client_id
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("X_CLIENT_ID not found. Set it in your environment or in .env"))
    }

    pub fn require_xai_key(&self) -> Result<&str> {
        self.xai_api_key
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("XAI_API_KEY not found. Set it in your environment or in .env"))
    }

    pub fn require_xai_management_key(&self) -> Result<&str> {
        self.xai_management_api_key
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("XAI_MANAGEMENT_API_KEY not found. Set it in your environment or in .env"))
    }

    pub fn cache_dir(&self) -> PathBuf {
        self.data_dir.join("cache")
    }

    pub fn exports_dir(&self) -> PathBuf {
        self.data_dir.join("exports")
    }

    pub fn snapshots_dir(&self) -> PathBuf {
        self.data_dir.join("snapshots")
    }

    pub fn tokens_path(&self) -> PathBuf {
        self.data_dir.join("oauth-tokens.json")
    }

    pub fn costs_path(&self) -> PathBuf {
        self.data_dir.join("api-costs.json")
    }

    pub fn watchlist_path(&self) -> PathBuf {
        self.data_dir.join("watchlist.json")
    }
}

fn resolve_data_dir() -> PathBuf {
    // Try relative to binary
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            let data = parent.join("data");
            if data.exists() {
                return data;
            }
        }
    }

    // Try current directory
    let cwd_data = PathBuf::from("data");
    if cwd_data.exists() {
        return cwd_data;
    }

    // Default: create in current directory
    cwd_data
}
