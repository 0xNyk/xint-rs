use anyhow::Result;
use serde::Serialize;
use std::path::Path;
use std::time::Instant;

use crate::costs;

#[derive(Debug, Clone, Serialize)]
pub struct OutputMeta {
    pub source: String,
    pub latency_ms: u128,
    pub cached: bool,
    pub confidence: f64,
    pub api_endpoint: String,
    pub timestamp: String,
    pub estimated_cost_usd: f64,
    pub budget_remaining_usd: f64,
}

fn round(value: f64, places: u32) -> f64 {
    let p = 10_f64.powi(places as i32);
    (value * p).round() / p
}

pub fn build_meta(
    source: &str,
    started_at: Instant,
    cached: bool,
    confidence: f64,
    api_endpoint: &str,
    estimated_cost_usd: f64,
    costs_path: &Path,
) -> OutputMeta {
    let budget = costs::check_budget(costs_path);
    OutputMeta {
        source: source.to_string(),
        latency_ms: started_at.elapsed().as_millis(),
        cached,
        confidence: round(confidence, 3),
        api_endpoint: api_endpoint.to_string(),
        timestamp: chrono::Utc::now().to_rfc3339(),
        estimated_cost_usd: round(estimated_cost_usd, 6),
        budget_remaining_usd: round(budget.remaining, 4),
    }
}

pub fn print_json_with_meta<T: Serialize>(meta: &OutputMeta, data: &T) -> Result<()> {
    let payload = serde_json::json!({
        "meta": meta,
        "data": data,
    });
    println!("{}", serde_json::to_string_pretty(&payload)?);
    Ok(())
}

pub fn print_jsonl_with_meta<T: Serialize>(
    meta: &OutputMeta,
    key: &str,
    items: &[T],
) -> Result<()> {
    for item in items {
        let mut obj = serde_json::to_value(meta)?;
        if let Some(map) = obj.as_object_mut() {
            map.insert(key.to_string(), serde_json::to_value(item)?);
        }
        println!("{}", serde_json::to_string(&obj)?);
    }
    Ok(())
}
