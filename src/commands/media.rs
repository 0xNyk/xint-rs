use anyhow::Result;
use serde::Serialize;
use std::fs;
use std::path::PathBuf;
use std::time::Duration;

use crate::cli::MediaArgs;
use crate::client::XClient;
use crate::config::Config;
use crate::costs;

#[derive(Debug, Serialize)]
struct DownloadRecord {
    media_key: String,
    media_type: String,
    source_url: Option<String>,
    saved_path: Option<String>,
    bytes: Option<usize>,
    error: Option<String>,
}

#[derive(Clone, Copy)]
enum MediaFilter {
    All,
    Photos,
    Video,
}

const DOWNLOAD_ATTEMPTS: u8 = 3;
const BACKOFF_BASE_MS: u64 = 500;
const DEFAULT_NAME_TEMPLATE: &str = "{tweet_id}-{index}-{type}";

struct FileNameContext<'a> {
    tweet_id: &'a str,
    username: Option<&'a str>,
    index: usize,
    media_type: &'a str,
    media_key: &'a str,
    created_at: Option<&'a str>,
    ext: &'a str,
}

pub async fn run(args: &MediaArgs, config: &Config, client: &XClient) -> Result<()> {
    if args.photos_only && args.video_only {
        anyhow::bail!("Use only one of --photos-only or --video-only.");
    }
    if let Some(max_items) = args.max_items {
        if max_items == 0 {
            anyhow::bail!("--max-items must be a positive integer.");
        }
    }
    if let Some(tpl) = &args.name_template {
        if tpl.trim().is_empty() {
            anyhow::bail!("--name-template must be a non-empty value.");
        }
    }

    let token = config.require_bearer_token()?;
    let tweet_id = extract_tweet_id(&args.target).ok_or_else(|| {
        anyhow::anyhow!("Invalid tweet ID or tweet URL. Expected numeric ID or status URL.")
    })?;
    let out_dir = resolve_output_dir(args.dir.as_ref(), config)?;

    let path = format!(
        "tweets/{tweet_id}?expansions=attachments.media_keys,author_id&tweet.fields=attachments,author_id,created_at&user.fields=username&media.fields=type,url,preview_image_url,variants,width,height,duration_ms,alt_text"
    );
    let raw = client.bearer_get(&path, token).await?;
    costs::track_cost(
        &config.costs_path(),
        "media_metadata",
        &format!("/2/tweets/{tweet_id}"),
        1,
    );

    let media_items = raw
        .includes
        .as_ref()
        .and_then(|i| i.media.as_ref())
        .cloned()
        .unwrap_or_default();
    let total_media = media_items.len();

    if total_media == 0 {
        println!("No media attachments found on this tweet.");
        return Ok(());
    }

    let media_filter = if args.photos_only {
        MediaFilter::Photos
    } else if args.video_only {
        MediaFilter::Video
    } else {
        MediaFilter::All
    };
    let mut selected_media: Vec<serde_json::Value> = media_items
        .into_iter()
        .filter(|m| media_matches_filter(m, media_filter))
        .collect();
    if let Some(max_items) = args.max_items {
        selected_media.truncate(max_items);
    }
    if selected_media.is_empty() {
        println!("No media attachments matched the selected filters.");
        return Ok(());
    }

    let author_id = raw
        .data
        .as_ref()
        .and_then(|d| d.get("author_id"))
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    let created_at = raw
        .data
        .as_ref()
        .and_then(|d| d.get("created_at"))
        .and_then(|v| v.as_str())
        .map(ToString::to_string);
    let username = raw
        .includes
        .as_ref()
        .and_then(|i| i.users.as_ref())
        .and_then(|users| {
            users
                .iter()
                .find(|u| u.id == author_id)
                .and_then(|u| u.username.clone())
        });

    let http = reqwest::Client::new();
    let mut records: Vec<DownloadRecord> = Vec::new();

    for (index, media) in selected_media.iter().enumerate() {
        let media_key = media
            .get("media_key")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string();
        let media_type = media
            .get("type")
            .and_then(|v| v.as_str())
            .unwrap_or("media")
            .to_string();
        let source_url = select_download_url(media);

        let base = DownloadRecord {
            media_key: media_key.clone(),
            media_type: media_type.clone(),
            source_url: source_url.clone(),
            saved_path: None,
            bytes: None,
            error: None,
        };

        let Some(url) = source_url else {
            records.push(DownloadRecord {
                error: Some("No downloadable URL found".to_string()),
                ..base
            });
            continue;
        };

        let bytes = match download_bytes_with_retry(&http, &url).await {
            Ok(b) => b,
            Err(err) => {
                records.push(DownloadRecord {
                    error: Some(err.to_string()),
                    ..base
                });
                continue;
            }
        };

        let ext = infer_extension(&url, &media_type);
        let file_name = build_file_name(
            args.name_template.as_deref(),
            &FileNameContext {
                tweet_id: &tweet_id,
                username: username.as_deref(),
                index: index + 1,
                media_type: &media_type,
                media_key: &media_key,
                created_at: created_at.as_deref(),
                ext: &ext,
            },
        );
        let file_path = out_dir.join(file_name);
        if let Err(err) = fs::write(&file_path, &bytes) {
            records.push(DownloadRecord {
                error: Some(err.to_string()),
                ..base
            });
            continue;
        }

        records.push(DownloadRecord {
            saved_path: Some(file_path.to_string_lossy().to_string()),
            bytes: Some(bytes.len()),
            ..base
        });
    }

    if args.json {
        let payload = serde_json::json!({
            "tweet_id": tweet_id,
            "username": username,
            "output_dir": out_dir.to_string_lossy(),
            "total_media": total_media,
            "selected_media": selected_media.len(),
            "name_template": args.name_template.as_deref().unwrap_or(DEFAULT_NAME_TEMPLATE),
            "downloaded": records.iter().filter(|r| r.error.is_none()).count(),
            "records": records,
        });
        println!("{}", serde_json::to_string_pretty(&payload)?);
        return Ok(());
    }

    println!(
        "\n🎞️  Media download for tweet {tweet_id}{}",
        username
            .as_ref()
            .map(|u| format!(" (@{u})"))
            .unwrap_or_default()
    );
    println!("Output directory: {}", out_dir.to_string_lossy());
    println!(
        "Attachments found: {} (selected: {})\n",
        total_media,
        selected_media.len()
    );

    for (idx, rec) in records.iter().enumerate() {
        if let Some(err) = &rec.error {
            println!(
                "{}. ❌ {} ({}) — {}",
                idx + 1,
                rec.media_type,
                rec.media_key,
                err
            );
        } else {
            println!("{}. ✅ {} ({})", idx + 1, rec.media_type, rec.media_key);
            if let (Some(path), Some(size)) = (&rec.saved_path, rec.bytes) {
                println!("   {path} ({size} bytes)");
            }
        }
    }

    let success = records.iter().filter(|r| r.error.is_none()).count();
    let failed = records.len().saturating_sub(success);
    println!("\nDone: {success} downloaded, {failed} failed.");

    Ok(())
}

fn is_video_like(media_type: &str) -> bool {
    media_type == "video" || media_type == "animated_gif"
}

fn sanitize_file_name(value: &str) -> String {
    let mut out = String::new();
    let mut last_was_underscore = false;
    for ch in value.chars() {
        let valid = ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.';
        if valid {
            out.push(ch);
            last_was_underscore = false;
        } else if !last_was_underscore {
            out.push('_');
            last_was_underscore = true;
        }
    }
    out.trim_matches('_').to_string()
}

fn has_extension(value: &str) -> bool {
    if let Some(dot) = value.rfind('.') {
        let ext = &value[dot + 1..];
        return (2..=5).contains(&ext.len()) && ext.chars().all(|c| c.is_ascii_alphanumeric());
    }
    false
}

fn build_file_name(template: Option<&str>, ctx: &FileNameContext<'_>) -> String {
    let raw_template = template
        .map(str::trim)
        .filter(|tpl| !tpl.is_empty())
        .unwrap_or(DEFAULT_NAME_TEMPLATE);

    let mut rendered = raw_template.to_string();
    let replacements = [
        ("{tweet_id}", sanitize_name_part(ctx.tweet_id)),
        (
            "{username}",
            sanitize_name_part(ctx.username.unwrap_or("unknown")),
        ),
        ("{index}", ctx.index.to_string()),
        ("{type}", sanitize_name_part(ctx.media_type)),
        ("{media_key}", sanitize_name_part(ctx.media_key)),
        (
            "{created_at}",
            sanitize_name_part(ctx.created_at.unwrap_or("unknown")),
        ),
        ("{ext}", sanitize_name_part(ctx.ext)),
    ];
    for (token, value) in replacements {
        rendered = rendered.replace(token, &value);
    }

    let mut base = sanitize_file_name(&rendered);
    if base.is_empty() {
        base = format!(
            "{}-{}-{}",
            sanitize_name_part(ctx.tweet_id),
            ctx.index,
            sanitize_name_part(ctx.media_type)
        );
    }

    if has_extension(&base) {
        base
    } else {
        format!("{}.{}", base, ctx.ext)
    }
}

fn media_matches_filter(media: &serde_json::Value, filter: MediaFilter) -> bool {
    let media_type = media
        .get("type")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    match filter {
        MediaFilter::All => true,
        MediaFilter::Photos => media_type == "photo",
        MediaFilter::Video => is_video_like(media_type),
    }
}

fn should_retry_status(status: u16) -> bool {
    status == 408 || status == 425 || status == 429 || status >= 500
}

async fn download_bytes_with_retry(http: &reqwest::Client, url: &str) -> Result<Vec<u8>> {
    let mut last_error = String::new();

    for attempt in 1..=DOWNLOAD_ATTEMPTS {
        match http.get(url).send().await {
            Ok(response) => {
                let status = response.status().as_u16();
                if response.status().is_success() {
                    let bytes = response.bytes().await?;
                    return Ok(bytes.to_vec());
                }

                let retryable = should_retry_status(status) && attempt < DOWNLOAD_ATTEMPTS;
                if !retryable {
                    anyhow::bail!("HTTP {status}");
                }
                last_error = format!("HTTP {status}");
            }
            Err(err) => {
                if attempt >= DOWNLOAD_ATTEMPTS {
                    return Err(err.into());
                }
                last_error = err.to_string();
            }
        }

        let delay_ms = BACKOFF_BASE_MS * (1_u64 << (attempt - 1));
        tokio::time::sleep(Duration::from_millis(delay_ms)).await;
    }

    anyhow::bail!("Download failed after retries: {last_error}")
}

fn resolve_output_dir(dir: Option<&String>, config: &Config) -> Result<PathBuf> {
    let base = match dir {
        Some(value) => {
            let p = PathBuf::from(value);
            if p.is_absolute() {
                p
            } else {
                std::env::current_dir()?.join(p)
            }
        }
        None => config.data_dir.join("media"),
    };

    fs::create_dir_all(&base)?;
    Ok(match base.canonicalize() {
        Ok(path) => path,
        Err(_) => base,
    })
}

fn extract_tweet_id(input: &str) -> Option<String> {
    let trimmed = input.trim();
    if trimmed.chars().all(|c| c.is_ascii_digit()) {
        return Some(trimmed.to_string());
    }

    let status_idx = trimmed.find("/status/")?;
    let id_part = &trimmed[status_idx + "/status/".len()..];
    let digits: String = id_part.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        None
    } else {
        Some(digits)
    }
}

fn sanitize_name_part(value: &str) -> String {
    let mut out = String::new();
    let mut last_was_underscore = false;
    for ch in value.chars() {
        let valid = ch.is_ascii_alphanumeric() || ch == '-' || ch == '_';
        if valid {
            out.push(ch);
            last_was_underscore = false;
        } else if !last_was_underscore {
            out.push('_');
            last_was_underscore = true;
        }
    }
    let trimmed = out.trim_matches('_').to_string();
    if trimmed.is_empty() {
        "media".to_string()
    } else {
        trimmed
    }
}

fn infer_extension(source_url: &str, media_type: &str) -> String {
    if let Ok(url) = url::Url::parse(source_url) {
        let path = url.path().to_lowercase();
        if let Some(dot) = path.rfind('.') {
            let ext = &path[dot + 1..];
            if (2..=5).contains(&ext.len()) && ext.chars().all(|c| c.is_ascii_alphanumeric()) {
                return ext.to_string();
            }
        }

        if let Some(format) = url.query_pairs().find_map(|(k, v)| {
            if k == "format" {
                Some(v.to_string())
            } else {
                None
            }
        }) {
            let lower = format.to_lowercase();
            if (2..=5).contains(&lower.len()) && lower.chars().all(|c| c.is_ascii_alphanumeric()) {
                return lower;
            }
        }
    }

    if media_type == "video" || media_type == "animated_gif" {
        "mp4".to_string()
    } else {
        "jpg".to_string()
    }
}

fn select_download_url(media: &serde_json::Value) -> Option<String> {
    let media_type = media
        .get("type")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    if media_type == "photo" {
        return media
            .get("url")
            .and_then(|v| v.as_str())
            .map(ToString::to_string)
            .or_else(|| {
                media
                    .get("preview_image_url")
                    .and_then(|v| v.as_str())
                    .map(ToString::to_string)
            });
    }

    if let Some(variants) = media.get("variants").and_then(|v| v.as_array()) {
        let mut best_url: Option<String> = None;
        let mut best_rate: i64 = -1;
        for variant in variants {
            let content_type = variant
                .get("content_type")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_lowercase();
            if !content_type.contains("mp4") {
                continue;
            }
            let Some(url) = variant.get("url").and_then(|v| v.as_str()) else {
                continue;
            };
            let bit_rate = variant
                .get("bit_rate")
                .and_then(|v| v.as_i64())
                .unwrap_or(0);
            if bit_rate > best_rate {
                best_rate = bit_rate;
                best_url = Some(url.to_string());
            }
        }
        if best_url.is_some() {
            return best_url;
        }
    }

    media
        .get("preview_image_url")
        .and_then(|v| v.as_str())
        .map(ToString::to_string)
        .or_else(|| {
            media
                .get("url")
                .and_then(|v| v.as_str())
                .map(ToString::to_string)
        })
}

#[cfg(test)]
mod tests {
    use super::{
        build_file_name, extract_tweet_id, infer_extension, media_matches_filter,
        select_download_url, should_retry_status, FileNameContext, MediaFilter,
    };

    #[test]
    fn extract_tweet_id_supports_id_and_url() {
        assert_eq!(
            extract_tweet_id("1900100012345678901"),
            Some("1900100012345678901".to_string())
        );
        assert_eq!(
            extract_tweet_id("https://x.com/user/status/1900100012345678901"),
            Some("1900100012345678901".to_string())
        );
        assert_eq!(extract_tweet_id("https://example.com"), None);
    }

    #[test]
    fn infer_extension_falls_back_by_media_type() {
        assert_eq!(
            infer_extension("https://pbs.twimg.com/media/abc", "photo"),
            "jpg"
        );
        assert_eq!(
            infer_extension("https://video.twimg.com/ext_tw_video/123", "video"),
            "mp4"
        );
    }

    #[test]
    fn select_download_url_prefers_highest_mp4_bitrate() {
        let media = serde_json::json!({
            "type": "video",
            "variants": [
                { "content_type": "video/mp4", "bit_rate": 256000, "url": "https://video/low.mp4" },
                { "content_type": "video/mp4", "bit_rate": 832000, "url": "https://video/high.mp4" }
            ]
        });
        assert_eq!(
            select_download_url(&media),
            Some("https://video/high.mp4".to_string())
        );
    }

    #[test]
    fn media_filter_matches_expected_types() {
        let photo = serde_json::json!({ "type": "photo" });
        let video = serde_json::json!({ "type": "video" });
        let gif = serde_json::json!({ "type": "animated_gif" });

        assert!(media_matches_filter(&photo, MediaFilter::Photos));
        assert!(!media_matches_filter(&video, MediaFilter::Photos));
        assert!(media_matches_filter(&video, MediaFilter::Video));
        assert!(media_matches_filter(&gif, MediaFilter::Video));
        assert!(!media_matches_filter(&photo, MediaFilter::Video));
    }

    #[test]
    fn retryable_statuses_are_identified() {
        assert!(should_retry_status(429));
        assert!(should_retry_status(503));
        assert!(!should_retry_status(404));
    }

    #[test]
    fn build_file_name_uses_placeholders_and_appends_ext() {
        let name = build_file_name(
            Some("{username}-{created_at}-{index}"),
            &FileNameContext {
                tweet_id: "1900100012345678901",
                username: Some("alice"),
                index: 2,
                media_type: "photo",
                media_key: "3_1",
                created_at: Some("2026-02-17T10:22:30.000Z"),
                ext: "jpg",
            },
        );
        assert_eq!(name, "alice-2026-02-17T10_22_30_000Z-2.jpg");
    }

    #[test]
    fn build_file_name_keeps_explicit_extension_placeholder() {
        let name = build_file_name(
            Some("{tweet_id}-{index}.{ext}"),
            &FileNameContext {
                tweet_id: "1900100012345678901",
                username: Some("alice"),
                index: 1,
                media_type: "video",
                media_key: "3_2",
                created_at: None,
                ext: "mp4",
            },
        );
        assert_eq!(name, "1900100012345678901-1.mp4");
    }
}
