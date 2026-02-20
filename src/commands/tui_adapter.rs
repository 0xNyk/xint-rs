use crate::action_result::{action_error, action_success, ActionExecutionResult};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TuiExecutionPlan {
    pub command: String,
    pub args: Vec<String>,
}

fn normalize_search_query(value: &str) -> String {
    value
        .split_whitespace()
        .map(|token| if token == "&" { "AND" } else { token })
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn build_tui_execution_plan(
    action_key: &str,
    value: Option<&str>,
) -> ActionExecutionResult<TuiExecutionPlan> {
    let normalized = value.unwrap_or("").trim();

    match action_key {
        "1" => {
            if normalized.is_empty() {
                return action_error("Query is required.");
            }
            let search_query = normalize_search_query(normalized);
            action_success(
                "Search plan ready.",
                Some(TuiExecutionPlan {
                    command: format!("xint search {search_query}"),
                    args: vec!["search".to_string(), search_query],
                }),
            )
        }
        "2" => {
            if normalized.is_empty() {
                return action_success(
                    "Trends plan ready.",
                    Some(TuiExecutionPlan {
                        command: "xint trends".to_string(),
                        args: vec!["trends".to_string()],
                    }),
                );
            }
            action_success(
                "Trends plan ready.",
                Some(TuiExecutionPlan {
                    command: format!("xint trends {normalized}"),
                    args: vec!["trends".to_string(), normalized.to_string()],
                }),
            )
        }
        "3" => {
            let username = normalized.trim_start_matches('@');
            if username.is_empty() {
                return action_error("Username is required.");
            }
            action_success(
                "Profile plan ready.",
                Some(TuiExecutionPlan {
                    command: format!("xint profile {username}"),
                    args: vec!["profile".to_string(), username.to_string()],
                }),
            )
        }
        "4" => {
            if normalized.is_empty() {
                return action_error("Tweet ID/URL is required.");
            }
            action_success(
                "Thread plan ready.",
                Some(TuiExecutionPlan {
                    command: format!("xint thread {normalized}"),
                    args: vec!["thread".to_string(), normalized.to_string()],
                }),
            )
        }
        "5" => {
            if normalized.is_empty() {
                return action_error("Article URL is required.");
            }
            action_success(
                "Article plan ready.",
                Some(TuiExecutionPlan {
                    command: format!("xint article {normalized}"),
                    args: vec!["article".to_string(), normalized.to_string()],
                }),
            )
        }
        "6" => action_success(
            "Help plan ready.",
            Some(TuiExecutionPlan {
                command: "xint --help".to_string(),
                args: vec!["--help".to_string()],
            }),
        ),
        _ => action_error(format!("Unsupported action key: {action_key}")),
    }
}

#[cfg(test)]
mod tests {
    use super::build_tui_execution_plan;

    #[test]
    fn builds_search_plan() {
        let result = build_tui_execution_plan("1", Some("ai agents"));
        assert_eq!(result.message, "Search plan ready.");
        let plan = result.data.expect("plan");
        assert_eq!(plan.command, "xint search ai agents");
        assert_eq!(
            plan.args,
            vec!["search".to_string(), "ai agents".to_string()]
        );
    }

    #[test]
    fn normalizes_ampersand_in_search_query() {
        let result = build_tui_execution_plan("1", Some("ai & solana"));
        assert_eq!(result.message, "Search plan ready.");
        let plan = result.data.expect("plan");
        assert_eq!(plan.command, "xint search ai AND solana");
        assert_eq!(
            plan.args,
            vec!["search".to_string(), "ai AND solana".to_string()]
        );
    }

    #[test]
    fn builds_trends_plan_for_blank_value() {
        let result = build_tui_execution_plan("2", Some(" "));
        assert_eq!(result.message, "Trends plan ready.");
        let plan = result.data.expect("plan");
        assert_eq!(plan.command, "xint trends");
        assert_eq!(plan.args, vec!["trends".to_string()]);
    }

    #[test]
    fn normalizes_profile_username() {
        let result = build_tui_execution_plan("3", Some("@nyk"));
        assert_eq!(result.message, "Profile plan ready.");
        let plan = result.data.expect("plan");
        assert_eq!(plan.command, "xint profile nyk");
        assert_eq!(plan.args, vec!["profile".to_string(), "nyk".to_string()]);
    }
}
