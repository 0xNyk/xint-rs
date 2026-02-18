use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;
use std::sync::{Mutex, OnceLock};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReliabilityMode {
    Cli,
    Mcp,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReliabilityEntry {
    pub timestamp: String,
    pub command: String,
    pub mode: ReliabilityMode,
    pub success: bool,
    pub latency_ms: u128,
    pub fallback: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ReliabilityData {
    pub entries: Vec<ReliabilityEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReliabilityCommandStats {
    pub calls: u64,
    pub success_rate: f64,
    pub error_rate: f64,
    pub fallback_rate: f64,
    pub p95_latency_ms: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReliabilityReport {
    pub generated_at: String,
    pub window_days: u32,
    pub total_calls: u64,
    pub success_rate: f64,
    pub by_command: HashMap<String, ReliabilityCommandStats>,
}

const RETENTION_DAYS: i64 = 30;
const MAX_ENTRIES: usize = 5000;

fn fallback_marks() -> &'static Mutex<HashSet<String>> {
    static STORE: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    STORE.get_or_init(|| Mutex::new(HashSet::new()))
}

fn round(value: f64, places: i32) -> f64 {
    let p = 10f64.powi(places);
    (value * p).round() / p
}

fn load_data(path: &Path) -> ReliabilityData {
    if !path.exists() {
        return ReliabilityData::default();
    }

    match fs::read_to_string(path) {
        Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
        Err(_) => ReliabilityData::default(),
    }
}

fn save_data(path: &Path, data: &ReliabilityData) {
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }

    let tmp = path.with_extension("json.tmp");
    if let Ok(json) = serde_json::to_string_pretty(data) {
        if fs::write(&tmp, json).is_ok() {
            let _ = fs::rename(&tmp, path);
        }
    }
}

fn prune(data: &mut ReliabilityData) {
    let cutoff = (chrono::Utc::now() - chrono::Duration::days(RETENTION_DAYS)).to_rfc3339();
    data.entries.retain(|entry| entry.timestamp >= cutoff);
    if data.entries.len() > MAX_ENTRIES {
        let drop_n = data.entries.len() - MAX_ENTRIES;
        data.entries.drain(0..drop_n);
    }
}

fn p95(latencies: &[u128]) -> f64 {
    if latencies.is_empty() {
        return 0.0;
    }
    let mut sorted = latencies.to_vec();
    sorted.sort_unstable();
    let idx = ((sorted.len() as f64) * 0.95).ceil() as usize;
    sorted[idx.saturating_sub(1)] as f64
}

pub fn mark_command_fallback(command: &str) {
    if command.is_empty() {
        return;
    }
    if let Ok(mut store) = fallback_marks().lock() {
        store.insert(command.to_string());
    }
}

pub fn consume_command_fallback(command: &str) -> bool {
    if command.is_empty() {
        return false;
    }
    if let Ok(mut store) = fallback_marks().lock() {
        return store.remove(command);
    }
    false
}

pub fn record_command_result(
    reliability_path: &Path,
    command: &str,
    success: bool,
    latency_ms: u128,
    mode: ReliabilityMode,
    fallback: bool,
) -> ReliabilityEntry {
    let entry = ReliabilityEntry {
        timestamp: chrono::Utc::now().to_rfc3339(),
        command: command.to_string(),
        mode,
        success,
        latency_ms,
        fallback,
    };

    let mut data = load_data(reliability_path);
    data.entries.push(entry.clone());
    prune(&mut data);
    save_data(reliability_path, &data);
    entry
}

pub fn get_reliability_report(reliability_path: &Path, window_days: u32) -> ReliabilityReport {
    let window_days = window_days.clamp(1, 30);
    let cutoff = (chrono::Utc::now() - chrono::Duration::days(window_days as i64)).to_rfc3339();

    let data = load_data(reliability_path);
    let recent: Vec<_> = data
        .entries
        .iter()
        .filter(|entry| entry.timestamp >= cutoff)
        .cloned()
        .collect();

    let mut grouped: HashMap<String, Vec<ReliabilityEntry>> = HashMap::new();
    for entry in &recent {
        grouped
            .entry(entry.command.clone())
            .or_default()
            .push(entry.clone());
    }

    let mut by_command = HashMap::new();
    for (command, entries) in grouped {
        let calls = entries.len() as u64;
        let successes = entries.iter().filter(|entry| entry.success).count() as u64;
        let fallbacks = entries.iter().filter(|entry| entry.fallback).count() as u64;
        let latencies: Vec<u128> = entries.iter().map(|entry| entry.latency_ms).collect();

        by_command.insert(
            command,
            ReliabilityCommandStats {
                calls,
                success_rate: round(
                    if calls > 0 {
                        successes as f64 / calls as f64
                    } else {
                        1.0
                    },
                    4,
                ),
                error_rate: round(
                    if calls > 0 {
                        (calls - successes) as f64 / calls as f64
                    } else {
                        0.0
                    },
                    4,
                ),
                fallback_rate: round(
                    if calls > 0 {
                        fallbacks as f64 / calls as f64
                    } else {
                        0.0
                    },
                    4,
                ),
                p95_latency_ms: round(p95(&latencies), 2),
            },
        );
    }

    let total_calls = recent.len() as u64;
    let total_successes = recent.iter().filter(|entry| entry.success).count() as u64;

    ReliabilityReport {
        generated_at: chrono::Utc::now().to_rfc3339(),
        window_days,
        total_calls,
        success_rate: round(
            if total_calls > 0 {
                total_successes as f64 / total_calls as f64
            } else {
                1.0
            },
            4,
        ),
        by_command,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn test_path() -> PathBuf {
        let path = PathBuf::from("/tmp/xint-rs-test-reliability.json");
        let _ = fs::remove_file(&path);
        path
    }

    #[test]
    fn records_and_reports_rates() {
        let path = test_path();
        let _ = record_command_result(&path, "search", true, 120, ReliabilityMode::Cli, false);
        let _ = record_command_result(&path, "search", false, 200, ReliabilityMode::Cli, false);
        let report = get_reliability_report(&path, 7);

        let stats = report
            .by_command
            .get("search")
            .expect("missing search stats");
        assert_eq!(stats.calls, 2);
        assert!((stats.success_rate - 0.5).abs() < 0.001);
        assert_eq!(stats.p95_latency_ms, 200.0);

        let _ = fs::remove_file(path);
    }

    #[test]
    fn fallback_markers_are_consumed_once() {
        mark_command_fallback("trends");
        assert!(consume_command_fallback("trends"));
        assert!(!consume_command_fallback("trends"));
    }
}
