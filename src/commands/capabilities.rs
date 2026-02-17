use anyhow::Result;
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;

use crate::cli::CapabilitiesArgs;
use crate::costs;

const PRICING_OPERATIONS: &[&str] = &[
    "bookmark_remove",
    "bookmark_save",
    "bookmarks",
    "blocks_add",
    "blocks_list",
    "blocks_remove",
    "follow",
    "followers",
    "following",
    "following_list",
    "like",
    "likes",
    "list_members_add",
    "list_members_list",
    "list_members_remove",
    "lists_create",
    "lists_delete",
    "lists_list",
    "lists_update",
    "media_metadata",
    "mutes_add",
    "mutes_list",
    "mutes_remove",
    "profile",
    "search",
    "search_archive",
    "stream_connect",
    "stream_rules_add",
    "stream_rules_delete",
    "stream_rules_list",
    "thread",
    "trends",
    "tweet",
    "unfollow",
    "unlike",
];

fn pricing_operations() -> Value {
    let mut map: BTreeMap<String, Value> = BTreeMap::new();

    for operation in PRICING_OPERATIONS {
        let (per_tweet, per_call) = costs::cost_rate(operation);
        map.insert(
            (*operation).to_string(),
            json!({
                "per_call_usd": per_call,
                "per_tweet_usd": per_tweet
            }),
        );
    }

    let mut out = Map::new();
    for (k, v) in map {
        out.insert(k, v);
    }
    Value::Object(out)
}

pub fn manifest() -> Value {
    json!({
      "schema_version": "1.0.0",
      "service": {
        "name": "xint-rs",
        "implementation": "rust",
        "version": env!("CARGO_PKG_VERSION"),
        "runtime": "cli",
        "protocols": ["cli", "mcp"]
      },
      "discovery": {
        "command": "xint capabilities --json",
        "content_type": "application/json",
        "supports_compact": true
      },
      "constraints": {
        "x_api_only": true,
        "xai_grok_only": true,
        "graphql": false,
        "session_cookies": false
      },
      "capability_modes": [
        {
          "mode": "read_only",
          "description": "Read/search/report operations with no account mutation."
        },
        {
          "mode": "engagement",
          "description": "Account actions (likes, follows, bookmarks, lists) via OAuth scopes."
        },
        {
          "mode": "moderation",
          "description": "Safety actions (block/mute) via OAuth scopes."
        }
      ],
      "telemetry": {
        "fields": ["source", "latency_ms", "cached", "confidence", "api_endpoint", "timestamp"],
        "budget_fields": ["estimated_cost_usd", "budget_remaining_usd"]
      },
      "pricing": {
        "model": "estimated_per_call_usd",
        "currency": "USD",
        "source": "local_cost_table",
        "operations": pricing_operations()
      },
      "policy": {
        "allowlist_default": "read_only",
        "enterprise_controls": ["mode_allowlist", "budget_guard", "oauth_scope_boundaries"]
      },
      "capabilities": [
        { "id": "search", "provider": "x_api_v2", "auth": "X_BEARER_TOKEN", "mode": "read_only" },
        { "id": "profile", "provider": "x_api_v2", "auth": "X_BEARER_TOKEN", "mode": "read_only" },
        { "id": "tweet", "provider": "x_api_v2", "auth": "X_BEARER_TOKEN", "mode": "read_only" },
        { "id": "thread", "provider": "x_api_v2", "auth": "X_BEARER_TOKEN", "mode": "read_only" },
        { "id": "trends", "provider": "x_api_v2", "auth": "X_BEARER_TOKEN", "mode": "read_only" },
        { "id": "article", "provider": "xai_grok", "auth": "XAI_API_KEY", "mode": "read_only" },
        { "id": "analyze", "provider": "xai_grok", "auth": "XAI_API_KEY", "mode": "read_only" },
        { "id": "report", "provider": "xai_grok", "auth": "X_BEARER_TOKEN+XAI_API_KEY", "mode": "read_only" },
        { "id": "x_search", "provider": "xai_grok", "auth": "XAI_API_KEY", "mode": "read_only" },
        { "id": "bookmarks", "provider": "x_api_v2", "auth": "OAuth2 PKCE", "mode": "engagement" },
        { "id": "likes", "provider": "x_api_v2", "auth": "OAuth2 PKCE", "mode": "engagement" },
        { "id": "follow", "provider": "x_api_v2", "auth": "OAuth2 PKCE", "mode": "engagement" },
        { "id": "lists", "provider": "x_api_v2", "auth": "OAuth2 PKCE", "mode": "engagement" },
        { "id": "blocks", "provider": "x_api_v2", "auth": "OAuth2 PKCE", "mode": "moderation" },
        { "id": "mutes", "provider": "x_api_v2", "auth": "OAuth2 PKCE", "mode": "moderation" },
        { "id": "stream", "provider": "x_api_v2", "auth": "X_BEARER_TOKEN", "mode": "read_only" },
        { "id": "mcp", "provider": "local_mcp", "auth": "none", "mode": "read_only" }
      ]
    })
}

pub fn run(args: &CapabilitiesArgs) -> Result<()> {
    let payload = manifest();
    if args.compact {
        println!("{}", serde_json::to_string(&payload)?);
    } else {
        println!("{}", serde_json::to_string_pretty(&payload)?);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::manifest;

    #[test]
    fn manifest_has_expected_constraints() {
        let payload = manifest();
        assert_eq!(payload["schema_version"], "1.0.0");
        assert_eq!(payload["constraints"]["x_api_only"], true);
        assert_eq!(payload["constraints"]["xai_grok_only"], true);
        assert_eq!(payload["constraints"]["graphql"], false);
        assert_eq!(payload["constraints"]["session_cookies"], false);
    }

    #[test]
    fn manifest_has_pricing_operations() {
        let payload = manifest();
        let ops = payload["pricing"]["operations"]
            .as_object()
            .expect("operations object");
        assert!(ops.contains_key("search"));
        assert!(ops.contains_key("trends"));
        assert!(ops.len() > 10);
    }
}
