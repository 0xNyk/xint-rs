#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionResultType {
    Success,
    Error,
    Confirm,
    Choice,
    Input,
    Info,
    Progress,
    Navigation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InteractiveAction {
    pub key: &'static str,
    pub label: &'static str,
    pub aliases: &'static [&'static str],
    pub hint: &'static str,
    pub summary: &'static str,
    pub example: &'static str,
    pub cost_hint: &'static str,
}

pub const INTERACTIVE_ACTIONS: &[InteractiveAction] = &[
    InteractiveAction {
        key: "1",
        label: "Search",
        aliases: &["search", "s"],
        hint: "keyword, topic, or boolean query",
        summary: "Discover relevant posts with ranked result quality.",
        example: "xint search \"open-source ai agents\"",
        cost_hint: "Low-medium (depends on query depth)",
    },
    InteractiveAction {
        key: "2",
        label: "Trends",
        aliases: &["trends", "trend", "t"],
        hint: "location name or blank for global",
        summary: "Surface current trend clusters globally or by location.",
        example: "xint trends \"San Francisco\"",
        cost_hint: "Low",
    },
    InteractiveAction {
        key: "3",
        label: "Profile",
        aliases: &["profile", "user", "p"],
        hint: "username (without @)",
        summary: "Inspect profile metadata and recent activity context.",
        example: "xint profile 0xNyk",
        cost_hint: "Low",
    },
    InteractiveAction {
        key: "4",
        label: "Thread",
        aliases: &["thread", "th"],
        hint: "tweet id or tweet url",
        summary: "Expand a tweet into threaded conversation context.",
        example: "xint thread https://x.com/.../status/...",
        cost_hint: "Medium",
    },
    InteractiveAction {
        key: "5",
        label: "Article",
        aliases: &["article", "a"],
        hint: "article url or tweet url",
        summary: "Fetch article content from URL or tweet-linked article.",
        example: "xint article https://x.com/.../status/...",
        cost_hint: "Medium-high (fetch + parse)",
    },
    InteractiveAction {
        key: "6",
        label: "Help",
        aliases: &["help", "h", "?"],
        hint: "show full CLI help",
        summary: "Display full command reference and flags.",
        example: "xint --help",
        cost_hint: "None",
    },
    InteractiveAction {
        key: "0",
        label: "Exit",
        aliases: &["exit", "quit", "q"],
        hint: "close interactive mode",
        summary: "Exit interactive dashboard.",
        example: "q",
        cost_hint: "None",
    },
];

pub fn normalize_interactive_choice(raw: &str) -> Option<&'static str> {
    let value = raw.trim().to_ascii_lowercase();
    if value.is_empty() {
        return None;
    }

    for action in INTERACTIVE_ACTIONS {
        if action.key == value {
            return Some(action.key);
        }
        if action
            .aliases
            .iter()
            .any(|alias| alias.eq_ignore_ascii_case(&value))
        {
            return Some(action.key);
        }
    }

    None
}

pub fn score_interactive_action(action: &InteractiveAction, query: &str) -> usize {
    let q = query.to_ascii_lowercase();
    if q.is_empty() {
        return 0;
    }

    let mut score = 0usize;
    if action.key == q {
        score += 100;
    }
    if action.label.eq_ignore_ascii_case(&q) {
        score += 90;
    }
    if action
        .aliases
        .iter()
        .any(|alias| alias.eq_ignore_ascii_case(&q))
    {
        score += 80;
    }
    if action.label.to_ascii_lowercase().starts_with(&q) {
        score += 70;
    }
    if action
        .aliases
        .iter()
        .any(|alias| alias.to_ascii_lowercase().starts_with(&q))
    {
        score += 60;
    }
    if action.label.to_ascii_lowercase().contains(&q) {
        score += 40;
    }
    if action.hint.to_ascii_lowercase().contains(&q) {
        score += 20;
    }

    score
}

#[cfg(test)]
mod tests {
    use super::{normalize_interactive_choice, score_interactive_action, INTERACTIVE_ACTIONS};

    #[test]
    fn normalize_choice_supports_numeric_and_alias_inputs() {
        assert_eq!(normalize_interactive_choice("1"), Some("1"));
        assert_eq!(normalize_interactive_choice("search"), Some("1"));
        assert_eq!(normalize_interactive_choice("Q"), Some("0"));
    }

    #[test]
    fn normalize_choice_rejects_invalid_values() {
        assert_eq!(normalize_interactive_choice(""), None);
        assert_eq!(normalize_interactive_choice("unknown"), None);
    }

    #[test]
    fn score_prioritizes_direct_key_matches() {
        let search = INTERACTIVE_ACTIONS
            .iter()
            .find(|action| action.key == "1")
            .expect("search action");
        let score = score_interactive_action(search, "1");
        assert!(score >= 100);
    }
}
