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
    ///
    /// Search order (first found wins per variable):
    /// 1. Already-set environment variables
    /// 2. `.env` in the current working directory
    /// 3. `.env` next to the binary
    /// 4. `~/.xint/.env` (canonical user config location)
    pub fn load() -> Result<Self> {
        // Try loading .env from current dir
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

        // Try ~/.xint/.env (canonical user config)
        if let Ok(home) = std::env::var("HOME") {
            let xint_env = PathBuf::from(home).join(".xint/.env");
            if xint_env.exists() {
                let _ = dotenvy::from_path(&xint_env);
            }
        }

        let bearer_token = non_empty_env("X_BEARER_TOKEN");
        let client_id = non_empty_env("X_CLIENT_ID");
        let xai_api_key = non_empty_env("XAI_API_KEY");
        let xai_management_api_key = non_empty_env("XAI_MANAGEMENT_API_KEY");

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
        self.bearer_token.as_deref().ok_or_else(|| {
            anyhow::anyhow!("X_BEARER_TOKEN not found. Set it in your environment or in .env")
        })
    }

    pub fn require_client_id(&self) -> Result<&str> {
        self.client_id.as_deref().ok_or_else(|| {
            anyhow::anyhow!("X_CLIENT_ID not found. Set it in your environment or in .env")
        })
    }

    pub fn require_xai_key(&self) -> Result<&str> {
        self.xai_api_key.as_deref().ok_or_else(|| {
            anyhow::anyhow!("XAI_API_KEY not found. Set it in your environment or in .env")
        })
    }

    pub fn require_xai_management_key(&self) -> Result<&str> {
        self.xai_management_api_key.as_deref().ok_or_else(|| {
            anyhow::anyhow!(
                "XAI_MANAGEMENT_API_KEY not found. Set it in your environment or in .env"
            )
        })
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

    pub fn reliability_path(&self) -> PathBuf {
        self.data_dir.join("reliability-metrics.json")
    }

    pub fn watchlist_path(&self) -> PathBuf {
        self.data_dir.join("watchlist.json")
    }
}

fn resolve_data_dir() -> PathBuf {
    // 1. Canonical user data dir: ~/.xint/data/ (highest priority)
    if let Ok(home) = std::env::var("HOME") {
        let xint_data = PathBuf::from(&home).join(".xint/data");
        if xint_data.exists() || std::fs::create_dir_all(&xint_data).is_ok() {
            return xint_data;
        }
    }

    // 2. Try relative to binary (brew cellar, development)
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            let data = parent.join("data");
            if data.exists() {
                return data;
            }
        }
    }

    // 3. Last resort: current directory
    PathBuf::from("data")
}

fn non_empty_env(key: &str) -> Option<String> {
    std::env::var(key)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}
