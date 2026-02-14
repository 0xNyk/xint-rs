use md5::{Digest, Md5};
use serde::{de::DeserializeOwned, Serialize};
use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};


#[derive(serde::Serialize, serde::Deserialize)]
struct CacheEntry<T> {
    query: String,
    params: String,
    timestamp: u64,
    data: T,
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn cache_key(query: &str, params: &str) -> String {
    let mut hasher = Md5::new();
    hasher.update(format!("{}|{}", query, params));
    let hash = hasher.finalize();
    hex::encode(&hash[..6]) // 12 hex chars
}

// We need hex encoding — implement inline to avoid a dep
mod hex {
    pub fn encode(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{:02x}", b)).collect()
    }
}

fn ensure_dir(dir: &Path) {
    if !dir.exists() {
        let _ = fs::create_dir_all(dir);
    }
}

/// Get a cached value. Returns None if expired or missing.
pub fn get<T: DeserializeOwned>(
    cache_dir: &Path,
    query: &str,
    params: &str,
    ttl_ms: u64,
) -> Option<T> {
    ensure_dir(cache_dir);
    let key = cache_key(query, params);
    let path = cache_dir.join(format!("{}.json", key));

    if !path.exists() {
        return None;
    }

    let content = fs::read_to_string(&path).ok()?;
    let entry: CacheEntry<T> = serde_json::from_str(&content).ok()?;

    if now_ms() - entry.timestamp > ttl_ms {
        let _ = fs::remove_file(&path);
        return None;
    }

    Some(entry.data)
}

/// Store a value in cache.
pub fn set<T: Serialize>(cache_dir: &Path, query: &str, params: &str, data: &T) {
    ensure_dir(cache_dir);
    let key = cache_key(query, params);
    let path = cache_dir.join(format!("{}.json", key));

    let entry = CacheEntry {
        query: query.to_string(),
        params: params.to_string(),
        timestamp: now_ms(),
        data,
    };

    if let Ok(json) = serde_json::to_string_pretty(&entry) {
        let _ = fs::write(&path, json);
    }
}

/// Prune expired entries. Returns count removed.
#[allow(dead_code)]
pub fn prune(cache_dir: &Path, ttl_ms: u64) -> usize {
    ensure_dir(cache_dir);
    let mut removed = 0;

    if let Ok(entries) = fs::read_dir(cache_dir) {
        let now = now_ms();
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "json") {
                if let Ok(content) = fs::read_to_string(&path) {
                    if let Ok(parsed) = serde_json::from_str::<CacheEntry<serde_json::Value>>(&content) {
                        if now - parsed.timestamp > ttl_ms {
                            let _ = fs::remove_file(&path);
                            removed += 1;
                        }
                    }
                }
            }
        }
    }

    removed
}

/// Clear all cache entries. Returns count removed.
pub fn clear(cache_dir: &Path) -> usize {
    ensure_dir(cache_dir);
    let mut removed = 0;

    if let Ok(entries) = fs::read_dir(cache_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "json") {
                if fs::remove_file(&path).is_ok() {
                    removed += 1;
                }
            }
        }
    }

    removed
}
