use crate::action_result::{action_error, action_success, ActionExecutionResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum McpToolRoute {
    Search,
    Profile,
    Thread,
    Tweet,
    Trends,
    XSearch,
    CollectionsList,
    Analyze,
    Article,
    CollectionsSearch,
    Bookmarks,
    PackageCreate,
    PackageStatus,
    PackageQuery,
    PackageRefresh,
    PackageSearch,
    PackagePublish,
    CacheClear,
    Watch,
    Diff,
    Report,
    Sentiment,
    Costs,
}

pub fn resolve_tool_route(name: &str) -> ActionExecutionResult<McpToolRoute> {
    match name {
        "xint_search" => action_success("tool route resolved", Some(McpToolRoute::Search)),
        "xint_profile" => action_success("tool route resolved", Some(McpToolRoute::Profile)),
        "xint_thread" => action_success("tool route resolved", Some(McpToolRoute::Thread)),
        "xint_tweet" => action_success("tool route resolved", Some(McpToolRoute::Tweet)),
        "xint_trends" => action_success("tool route resolved", Some(McpToolRoute::Trends)),
        "xint_xsearch" => action_success("tool route resolved", Some(McpToolRoute::XSearch)),
        "xint_collections_list" => {
            action_success("tool route resolved", Some(McpToolRoute::CollectionsList))
        }
        "xint_analyze" => action_success("tool route resolved", Some(McpToolRoute::Analyze)),
        "xint_article" => action_success("tool route resolved", Some(McpToolRoute::Article)),
        "xint_collections_search" => {
            action_success("tool route resolved", Some(McpToolRoute::CollectionsSearch))
        }
        "xint_bookmarks" => action_success("tool route resolved", Some(McpToolRoute::Bookmarks)),
        "xint_package_create" => {
            action_success("tool route resolved", Some(McpToolRoute::PackageCreate))
        }
        "xint_package_status" => {
            action_success("tool route resolved", Some(McpToolRoute::PackageStatus))
        }
        "xint_package_query" => {
            action_success("tool route resolved", Some(McpToolRoute::PackageQuery))
        }
        "xint_package_refresh" => {
            action_success("tool route resolved", Some(McpToolRoute::PackageRefresh))
        }
        "xint_package_search" => {
            action_success("tool route resolved", Some(McpToolRoute::PackageSearch))
        }
        "xint_package_publish" => {
            action_success("tool route resolved", Some(McpToolRoute::PackagePublish))
        }
        "xint_cache_clear" => action_success("tool route resolved", Some(McpToolRoute::CacheClear)),
        "xint_watch" => action_success("tool route resolved", Some(McpToolRoute::Watch)),
        "xint_diff" => action_success("tool route resolved", Some(McpToolRoute::Diff)),
        "xint_report" => action_success("tool route resolved", Some(McpToolRoute::Report)),
        "xint_sentiment" => action_success("tool route resolved", Some(McpToolRoute::Sentiment)),
        "xint_costs" => action_success("tool route resolved", Some(McpToolRoute::Costs)),
        _ => action_error(format!("Unknown tool: {name}")),
    }
}

#[cfg(test)]
mod tests {
    use super::{resolve_tool_route, McpToolRoute};

    #[test]
    fn resolves_known_tools() {
        assert_eq!(
            resolve_tool_route("xint_search").data,
            Some(McpToolRoute::Search)
        );
        assert_eq!(
            resolve_tool_route("xint_package_query").data,
            Some(McpToolRoute::PackageQuery)
        );
    }

    #[test]
    fn rejects_unknown_tools() {
        assert!(resolve_tool_route("xint_unknown").data.is_none());
    }
}
