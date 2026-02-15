use anyhow::{bail, Result};
use std::path::PathBuf;

use crate::api::xai;
use crate::cli::CollectionsArgs;
use crate::config::Config;

pub async fn run(args: &CollectionsArgs, config: &Config) -> Result<()> {
    let http = reqwest::Client::new();
    let parts: Vec<String> = args.subcommand.clone().unwrap_or_default();
    let sub = parts.first().map(|s| s.as_str()).unwrap_or("help");

    match sub {
        "list" => cmd_list(&http, config).await,
        "create" => cmd_create(&http, config, &parts).await,
        "ensure" => cmd_ensure(&http, config, &parts).await,
        "add-document" | "add-doc" => cmd_add_document(&http, config, &parts).await,
        "upload" => cmd_upload(&http, config, &parts).await,
        "search" => cmd_search(&http, config, &parts, args).await,
        "sync-dir" | "sync" => cmd_sync_dir(&http, config, args).await,
        "help" | "--help" | "-h" => {
            print_help();
            Ok(())
        }
        _ => {
            eprintln!("Unknown collections subcommand: {}", sub);
            print_help();
            Ok(())
        }
    }
}

async fn cmd_list(http: &reqwest::Client, config: &Config) -> Result<()> {
    let key = config.require_xai_management_key()?;
    let res = xai::collections_list(http, key).await?;
    println!("{}", serde_json::to_string_pretty(&res)?);
    Ok(())
}

async fn cmd_create(http: &reqwest::Client, config: &Config, parts: &[String]) -> Result<()> {
    let key = config.require_xai_management_key()?;
    let name = find_flag(parts, "--name")
        .ok_or_else(|| anyhow::anyhow!("--name required for 'collections create'"))?;
    let desc = find_flag(parts, "--description").unwrap_or_default();
    let res = xai::collections_create(http, key, &name, &desc).await?;
    println!("{}", serde_json::to_string_pretty(&res)?);
    Ok(())
}

async fn cmd_ensure(http: &reqwest::Client, config: &Config, parts: &[String]) -> Result<()> {
    let key = config.require_xai_management_key()?;
    let name = find_flag(parts, "--name")
        .ok_or_else(|| anyhow::anyhow!("--name required for 'collections ensure'"))?;
    let desc = find_flag(parts, "--description").unwrap_or_default();
    let res = xai::collections_ensure(http, key, &name, &desc).await?;
    println!("{}", serde_json::to_string_pretty(&res)?);
    Ok(())
}

async fn cmd_add_document(
    http: &reqwest::Client,
    config: &Config,
    parts: &[String],
) -> Result<()> {
    let key = config.require_xai_management_key()?;
    let collection_id = find_flag(parts, "--collection-id")
        .ok_or_else(|| anyhow::anyhow!("--collection-id required"))?;
    let document_id = find_flag(parts, "--document-id")
        .ok_or_else(|| anyhow::anyhow!("--document-id required"))?;
    let res = xai::collections_add_document(http, key, &collection_id, &document_id).await?;
    println!("{}", serde_json::to_string_pretty(&res)?);
    Ok(())
}

async fn cmd_upload(http: &reqwest::Client, config: &Config, parts: &[String]) -> Result<()> {
    let api_key = config.require_xai_key()?;
    let path_str =
        find_flag(parts, "--path").ok_or_else(|| anyhow::anyhow!("--path required for upload"))?;
    let purpose = find_flag(parts, "--purpose").unwrap_or_else(|| "kb_sync".to_string());

    let path = PathBuf::from(&path_str);
    if !path.exists() {
        bail!("File not found: {}", path_str);
    }

    let res = xai::files_upload(http, api_key, &path, &purpose).await?;
    println!("{}", serde_json::to_string_pretty(&res)?);
    Ok(())
}

async fn cmd_search(
    http: &reqwest::Client,
    config: &Config,
    parts: &[String],
    args: &CollectionsArgs,
) -> Result<()> {
    let api_key = config.require_xai_key()?;
    let query = find_flag(parts, "--query")
        .ok_or_else(|| anyhow::anyhow!("--query required for search"))?;
    let cids_str = find_flag(parts, "--collection-ids").unwrap_or_default();
    let collection_ids: Vec<String> = cids_str
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    let top_k = args.top_k;

    let res = xai::documents_search(http, api_key, &collection_ids, &query, top_k).await?;
    println!("{}", serde_json::to_string_pretty(&res)?);
    Ok(())
}

async fn cmd_sync_dir(
    http: &reqwest::Client,
    config: &Config,
    args: &CollectionsArgs,
) -> Result<()> {
    let api_key = config.require_xai_key()?;
    let mgmt_key = config.require_xai_management_key()?;
    let parts: Vec<String> = args.subcommand.clone().unwrap_or_default();

    let collection_name = find_flag(&parts, "--collection-name")
        .or_else(|| find_flag(&parts, "--name"))
        .ok_or_else(|| anyhow::anyhow!("--collection-name required for sync-dir"))?;
    let dir_str = find_flag(&parts, "--dir")
        .ok_or_else(|| anyhow::anyhow!("--dir required for sync-dir"))?;
    let directory = PathBuf::from(&dir_str);
    if !directory.exists() {
        bail!("Directory not found: {}", dir_str);
    }

    let globs: Vec<String> = {
        let g = find_all_flags(&parts, "--glob");
        if g.is_empty() {
            vec!["*.md".to_string()]
        } else {
            g
        }
    };
    let limit: usize = find_flag(&parts, "--limit")
        .and_then(|v| v.parse().ok())
        .unwrap_or(50);
    let purpose = find_flag(&parts, "--purpose").unwrap_or_else(|| "kb_sync".to_string());

    let ts = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();

    // 1) Ensure collection
    println!("Ensuring collection '{}'...", collection_name);
    let ensure_res = xai::collections_ensure(http, mgmt_key, &collection_name, "xint KB sync").await?;

    let collection_id = extract_collection_id(&ensure_res);
    if collection_id.is_empty() {
        eprintln!("WARN: could not determine collection_id from response; add-document step may fail.");
    }

    // 2) Enumerate files
    let mut paths: Vec<PathBuf> = Vec::new();
    for g in &globs {
        if let Ok(entries) = glob::glob(&directory.join(g).to_string_lossy()) {
            for entry in entries.flatten() {
                if entry.is_file() {
                    paths.push(entry);
                }
            }
        } else {
            // Fallback: manual glob matching
            if let Ok(entries) = std::fs::read_dir(&directory) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_file() {
                        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                        if matches_simple_glob(name, g) {
                            paths.push(path);
                        }
                    }
                }
            }
        }
    }

    // Dedupe
    let mut seen = std::collections::HashSet::new();
    paths.retain(|p| {
        let key = p.to_string_lossy().to_string();
        seen.insert(key)
    });
    paths.sort();
    paths.truncate(limit);

    println!("Found {} files matching globs", paths.len());

    // 3) Upload + attach
    let mut uploaded = 0;
    let mut attached = 0;
    let mut failures: Vec<String> = Vec::new();

    for p in &paths {
        let fname = p.file_name().and_then(|n| n.to_str()).unwrap_or("?");
        print!("  Uploading {}... ", fname);

        match xai::files_upload(http, api_key, p, &purpose).await {
            Ok(up_res) => {
                uploaded += 1;
                let doc_id = extract_document_id(&up_res);

                if !collection_id.is_empty() && !doc_id.is_empty() {
                    match xai::collections_add_document(http, mgmt_key, &collection_id, &doc_id)
                        .await
                    {
                        Ok(_) => {
                            attached += 1;
                            println!("uploaded + attached");
                        }
                        Err(e) => {
                            println!("uploaded (attach failed: {})", e);
                            failures.push(format!("{}: attach error: {}", fname, e));
                        }
                    }
                } else {
                    println!("uploaded (no collection_id or doc_id to attach)");
                }
            }
            Err(e) => {
                println!("FAILED: {}", e);
                failures.push(format!("{}: {}", fname, e));
            }
        }
    }

    // 4) Write report
    let report_path = find_flag(&parts, "--report-md")
        .map(PathBuf::from)
        .unwrap_or_else(|| directory.join("xai-collections-sync-latest.md"));

    let mut lines = Vec::new();
    lines.push("# xAI Collections KB Sync".to_string());
    lines.push(String::new());
    lines.push(format!("- Timestamp (UTC): {}", ts));
    lines.push(format!("- Collection name: `{}`", collection_name));
    lines.push(format!("- Directory: `{}`", directory.display()));
    lines.push(format!("- Globs: `{}`", globs.join(", ")));
    lines.push(format!("- Limit: {}", limit));
    lines.push(String::new());
    lines.push("## Summary".to_string());
    lines.push(String::new());
    lines.push(format!("- Files considered: {}", paths.len()));
    lines.push(format!("- Upload attempts: {}", uploaded));
    lines.push(format!("- Attach attempts: {}", attached));
    lines.push(format!("- Failures: {}", failures.len()));
    lines.push(String::new());
    if !failures.is_empty() {
        lines.push("## Failures".to_string());
        lines.push(String::new());
        for f in &failures[..failures.len().min(50)] {
            lines.push(format!("- {}", f));
        }
        lines.push(String::new());
    }

    if let Some(parent) = report_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&report_path, lines.join("\n").trim_end().to_owned() + "\n")?;

    println!(
        "\nSync complete: {} uploaded, {} attached, {} failures",
        uploaded,
        attached,
        failures.len()
    );
    println!("Report: {}", report_path.display());

    Ok(())
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn find_flag(parts: &[String], flag: &str) -> Option<String> {
    for (i, p) in parts.iter().enumerate() {
        if p == flag {
            if let Some(val) = parts.get(i + 1) {
                return Some(val.clone());
            }
        }
        if let Some(rest) = p.strip_prefix(&format!("{}=", flag)) {
            return Some(rest.to_string());
        }
    }
    None
}

fn find_all_flags(parts: &[String], flag: &str) -> Vec<String> {
    let mut vals = Vec::new();
    for (i, p) in parts.iter().enumerate() {
        if p == flag {
            if let Some(val) = parts.get(i + 1) {
                vals.push(val.clone());
            }
        }
        if let Some(rest) = p.strip_prefix(&format!("{}=", flag)) {
            vals.push(rest.to_string());
        }
    }
    vals
}

fn extract_collection_id(res: &serde_json::Value) -> String {
    // Try collection.id or created.data.id
    if let Some(coll) = res.get("collection") {
        for key in &["id", "collection_id"] {
            if let Some(id) = coll.get(key).and_then(|v| v.as_str()) {
                let s = id.trim();
                if !s.is_empty() {
                    return s.to_string();
                }
            }
        }
    }
    if let Some(created) = res.get("created") {
        if let Some(data) = created.get("data") {
            for key in &["id", "collection_id"] {
                if let Some(id) = data.get(key).and_then(|v| v.as_str()) {
                    let s = id.trim();
                    if !s.is_empty() {
                        return s.to_string();
                    }
                }
            }
        }
        for key in &["id", "collection_id"] {
            if let Some(id) = created.get(key).and_then(|v| v.as_str()) {
                let s = id.trim();
                if !s.is_empty() {
                    return s.to_string();
                }
            }
        }
    }
    String::new()
}

fn extract_document_id(res: &serde_json::Value) -> String {
    if let Some(data) = res.get("data") {
        for key in &["id", "file_id", "document_id"] {
            if let Some(id) = data.get(key).and_then(|v| v.as_str()) {
                let s = id.trim();
                if !s.is_empty() {
                    return s.to_string();
                }
            }
        }
    }
    for key in &["id", "file_id", "document_id"] {
        if let Some(id) = res.get(key).and_then(|v| v.as_str()) {
            let s = id.trim();
            if !s.is_empty() {
                return s.to_string();
            }
        }
    }
    String::new()
}

fn matches_simple_glob(name: &str, pattern: &str) -> bool {
    if let Some(ext) = pattern.strip_prefix("*.") {
        name.ends_with(&format!(".{}", ext))
    } else {
        name == pattern
    }
}

fn print_help() {
    println!("xAI Collections Knowledge Base");
    println!();
    println!("Usage: xint collections <subcommand> [options]");
    println!();
    println!("Subcommands:");
    println!("  list                                List all collections");
    println!("  create --name <n> [--description <d>]  Create a collection");
    println!("  ensure --name <n>                   Create collection if it doesn't exist");
    println!("  add-document --collection-id <id> --document-id <id>");
    println!("  upload --path <file> [--purpose <p>] Upload a file");
    println!("  search --query <q> [--collection-ids <id1,id2>]");
    println!("  sync-dir --collection-name <n> --dir <path> [--glob *.md] [--limit 50]");
    println!();
    println!("Env vars:");
    println!("  XAI_API_KEY              File upload + document search");
    println!("  XAI_MANAGEMENT_API_KEY   Collections management");
}
