#[cfg(test)]
mod tests {
    use crate::costs::{cost_rate, track_cost, check_budget, set_budget, get_cost_summary};
    use std::path::PathBuf;
    use std::fs;

    fn test_data_path() -> PathBuf {
        let path = PathBuf::from("/tmp/xint-rs-test-costs.json");
        let _ = fs::remove_file(&path);
        path
    }

    #[test]
    fn test_cost_rate_search() {
        let (per_tweet, per_call) = cost_rate("search");
        assert_eq!(per_tweet, 0.005);
        assert_eq!(per_call, 0.0);
    }

    #[test]
    fn test_cost_rate_like() {
        let (per_tweet, per_call) = cost_rate("like");
        assert_eq!(per_tweet, 0.0);
        assert_eq!(per_call, 0.01);
    }

    #[test]
    fn test_cost_rate_trends() {
        let (per_tweet, per_call) = cost_rate("trends");
        assert_eq!(per_tweet, 0.0);
        assert_eq!(per_call, 0.10);
    }

    #[test]
    fn test_cost_rate_unknown() {
        let (per_tweet, per_call) = cost_rate("unknown_operation");
        assert_eq!(per_tweet, 0.005); // fallback
        assert_eq!(per_call, 0.0);
    }

    #[test]
    fn test_track_cost() {
        let path = test_data_path();
        
        let entry = track_cost(&path, "search", "/2/tweets/search/recent", 10);
        
        assert_eq!(entry.operation, "search");
        assert_eq!(entry.tweets_read, 10);
        assert!((entry.cost_usd - 0.05).abs() < 0.001);
        
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn test_check_budget() {
        let path = test_data_path();
        
        let status = check_budget(&path);
        
        assert!(status.limit > 0.0);
        assert!(status.spent >= 0.0);
        
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn test_set_budget() {
        let path = test_data_path();
        
        set_budget(&path, 5.0);
        let status = check_budget(&path);
        
        assert_eq!(status.limit, 5.0);
        
        // Restore default
        set_budget(&path, 1.0);
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn test_get_cost_summary_today() {
        let path = test_data_path();
        
        // Track some costs first
        let _ = track_cost(&path, "search", "/test", 10);
        
        let summary = get_cost_summary(&path, "today");
        
        assert!(summary.contains("API Costs"));
        assert!(summary.contains("Today"));
        
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn test_get_cost_summary_week() {
        let path = test_data_path();
        
        let summary = get_cost_summary(&path, "week");
        
        assert!(summary.contains("API Costs"));
        
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn test_get_cost_summary_month() {
        let path = test_data_path();
        
        let summary = get_cost_summary(&path, "month");
        
        assert!(summary.contains("API Costs"));
        
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn test_get_cost_summary_all() {
        let path = test_data_path();
        
        let summary = get_cost_summary(&path, "all");
        
        assert!(summary.contains("API Costs"));
        assert!(summary.contains("All Time"));
        
        let _ = fs::remove_file(&path);
    }
}
