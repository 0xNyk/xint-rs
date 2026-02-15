use anyhow::{bail, Result};
use std::collections::HashSet;
use std::path::PathBuf;

use crate::api::xai;
use crate::cli::XSearchArgs;
use crate::config::Config;

pub async fn run(args: &XSearchArgs, config: &Config) -> Result<()> {
    let api_key = config.require_xai_key()?;
    let http = reqwest::Client::new();

    // Load queries
    let queries = load_queries(&args.queries_file)?;
    if queries.is_empty() {
        bail!("No queries provided.");
    }

    let ts = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let model = &args.model;
    let max_results = args.max_results;
    let timeout = args.timeout_seconds as u64;
    let from_date = args.from_date.as_deref();
    let to_date = args.to_date.as_deref();

    let mut per_query: Vec<QueryResult> = Vec::new();
    let mut had_errors = false;

    for q in &queries {
        match xai::x_search(
            &http,
            api_key,
            q,
            max_results,
            from_date,
            to_date,
            model,
            timeout,
        )
        .await
        {
            Ok((results, summary)) => {
                per_query.push(QueryResult {
                    query: q.clone(),
                    results,
                    summary,
                    error: None,
                });
            }
            Err(e) => {
                had_errors = true;
                let err_msg = format!("{}", e);
                per_query.push(QueryResult {
                    query: q.clone(),
                    results: Vec::new(),
                    summary: String::new(),
                    error: Some(err_msg[..err_msg.len().min(500)].to_string()),
                });
            }
        }
    }

    // Build JSON payload
    let json_payload = build_json_payload(&ts, model, max_results, from_date, to_date, &per_query);

    // Write JSON output
    let out_json = PathBuf::from(&args.out_json);
    if let Some(parent) = out_json.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&out_json, serde_json::to_string_pretty(&json_payload)? + "\n")?;

    // Write markdown report
    let combined_summary = per_query
        .iter()
        .rev()
        .find_map(|qr| {
            if qr.summary.is_empty() {
                None
            } else {
                Some(qr.summary.clone())
            }
        })
        .unwrap_or_default();

    let md = render_md(&ts, &queries, &per_query, &combined_summary);
    let out_md = PathBuf::from(&args.out_md);
    if let Some(parent) = out_md.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&out_md, &md)?;

    // Print status
    let total_results: usize = per_query.iter().map(|qr| qr.results.len()).sum();
    let status = if had_errors && total_results == 0 {
        "FAIL"
    } else if had_errors {
        "PARTIAL"
    } else {
        "OK"
    };
    println!(
        "xAI X search: {} ({} queries, {} results)",
        status,
        queries.len(),
        total_results
    );
    println!("Report: {}", out_md.display());

    // Emit memory candidates if requested
    if args.emit_candidates {
        let workspace = PathBuf::from(&args.workspace);
        let existing = load_existing_sources(&workspace);
        let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
        let cand_out = if args.candidates_out.is_empty() {
            workspace
                .join("memory")
                .join("candidates")
                .join(format!("x-search-{}.jsonl", today))
        } else {
            PathBuf::from(&args.candidates_out)
        };
        if let Some(parent) = cand_out.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let mut new_lines = Vec::new();
        let mut seen = existing;
        for qr in &per_query {
            if qr.error.is_some() {
                continue;
            }
            for r in &qr.results {
                let url = r.best_url();
                let text = r.best_text().replace('\n', " ");
                let handle = r.best_handle();
                if text.is_empty() && url.is_empty() {
                    continue;
                }
                let source = if !url.is_empty() {
                    format!("x_search:{}", url)
                } else {
                    use std::hash::{Hash, Hasher};
                    let mut hasher = std::collections::hash_map::DefaultHasher::new();
                    text.hash(&mut hasher);
                    format!("x_search:{}:{}", handle, hasher.finish())
                };
                if seen.contains(&source) {
                    continue;
                }

                let mut val = text.clone();
                if !handle.is_empty() {
                    val = format!("@{}: {}", handle, val);
                }
                if !url.is_empty() {
                    val = format!("{} ({})", val, url);
                }
                if val.len() > 900 {
                    val.truncate(900);
                }

                let item = serde_json::json!({
                    "type": "fact",
                    "value": val,
                    "confidence": 0.55,
                    "source": source,
                    "created_at": ts,
                });
                new_lines.push(serde_json::to_string(&item)?);
                seen.insert(source);
            }
        }

        if !new_lines.is_empty() {
            use std::io::Write;
            let mut f = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&cand_out)?;
            for line in &new_lines {
                writeln!(f, "{}", line)?;
            }
        }
        println!("New memory candidates: {}", new_lines.len());
        if !new_lines.is_empty() {
            println!("Candidates appended: {}", cand_out.display());
        }
    }

    Ok(())
}

struct QueryResult {
    query: String,
    results: Vec<xai::XSearchResult>,
    summary: String,
    error: Option<String>,
}

fn load_queries(path: &str) -> Result<Vec<String>> {
    let content = std::fs::read_to_string(path)?;
    let val: serde_json::Value = serde_json::from_str(&content)?;

    if let Some(arr) = val.get("queries").and_then(|v| v.as_array()) {
        Ok(arr
            .iter()
            .filter_map(|v| v.as_str().map(|s| s.trim().to_string()))
            .filter(|s| !s.is_empty())
            .collect())
    } else if let Some(arr) = val.as_array() {
        Ok(arr
            .iter()
            .filter_map(|v| v.as_str().map(|s| s.trim().to_string()))
            .filter(|s| !s.is_empty())
            .collect())
    } else {
        bail!("queries file must be a JSON array of strings or {{\"queries\": [...]}} object")
    }
}

fn build_json_payload(
    ts: &str,
    model: &str,
    max_results: u32,
    from_date: Option<&str>,
    to_date: Option<&str>,
    per_query: &[QueryResult],
) -> serde_json::Value {
    let queries: Vec<serde_json::Value> = per_query
        .iter()
        .map(|qr| {
            serde_json::json!({
                "query": qr.query,
                "summary": qr.summary,
                "results": qr.results.iter().map(|r| {
                    serde_json::json!({
                        "url": r.best_url(),
                        "text": r.best_text(),
                        "username": r.best_handle(),
                        "created_at": r.best_created_at(),
                    })
                }).collect::<Vec<_>>(),
                "error": qr.error,
            })
        })
        .collect();

    serde_json::json!({
        "timestamp": ts,
        "model": model,
        "max_results": max_results,
        "from_date": from_date,
        "to_date": to_date,
        "queries": queries,
    })
}

fn render_md(ts: &str, queries: &[String], per_query: &[QueryResult], summary: &str) -> String {
    let mut lines = Vec::new();
    lines.push("# xAI X Search Scan".to_string());
    lines.push(String::new());
    lines.push(format!("- Timestamp (UTC): {}", ts));
    lines.push(format!("- Queries: {}", queries.len()));
    lines.push(String::new());

    if !summary.is_empty() {
        lines.push("## Summary".to_string());
        lines.push(String::new());
        lines.push(summary.trim().to_string());
        lines.push(String::new());
    }

    lines.push("## Results".to_string());
    lines.push(String::new());

    for qr in per_query {
        lines.push(format!("### Query: `{}`", qr.query));
        lines.push(String::new());
        if let Some(err) = &qr.error {
            lines.push(format!("- ERROR: {}", err));
            lines.push(String::new());
            continue;
        }
        if qr.results.is_empty() {
            lines.push("- (no results)".to_string());
            lines.push(String::new());
            continue;
        }
        for r in &qr.results {
            let handle = r.best_handle();
            let mut text = r.best_text().replace('\n', " ");
            if text.len() > 220 {
                text.truncate(217);
                text.push_str("...");
            }
            let url = r.best_url();
            let created_at = r.best_created_at();

            let prefix = if handle.is_empty() {
                String::new()
            } else {
                format!("@{}: ", handle)
            };
            let meta = if created_at.is_empty() {
                String::new()
            } else {
                format!(" ({})", created_at)
            };

            if url.is_empty() {
                lines.push(format!("- {}{}{}", prefix, text, meta));
            } else {
                lines.push(format!("- {}{}{} {}", prefix, text, meta, url));
            }
        }
        lines.push(String::new());
    }

    lines.join("\n").trim_end().to_string() + "\n"
}

fn load_existing_sources(workspace: &std::path::Path) -> HashSet<String> {
    let mut sources = HashSet::new();

    // Check candidates
    let cand_dir = workspace.join("memory").join("candidates");
    if cand_dir.exists() {
        if let Ok(entries) = std::fs::read_dir(&cand_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) == Some("jsonl") {
                    if let Ok(content) = std::fs::read_to_string(&path) {
                        for line in content.lines() {
                            let line = line.trim();
                            if line.is_empty() {
                                continue;
                            }
                            if let Ok(obj) = serde_json::from_str::<serde_json::Value>(line) {
                                if let Some(s) = obj.get("source").and_then(|v| v.as_str()) {
                                    sources.insert(s.to_string());
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // Check store
    let store_dir = workspace.join("memory").join("store");
    for name in &["provisional.jsonl", "confirmed.jsonl", "rejected.jsonl"] {
        let p = store_dir.join(name);
        if let Ok(content) = std::fs::read_to_string(&p) {
            for line in content.lines() {
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }
                if let Ok(obj) = serde_json::from_str::<serde_json::Value>(line) {
                    if let Some(s) = obj.get("source").and_then(|v| v.as_str()) {
                        sources.insert(s.to_string());
                    }
                }
            }
        }
    }

    sources
}
