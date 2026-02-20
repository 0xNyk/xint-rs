#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionResultType {
    Success,
    Error,
    Info,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionExecutionResult<T> {
    pub result_type: ActionResultType,
    pub message: String,
    pub data: Option<T>,
    pub fallback_used: bool,
}

pub fn action_success<T>(message: impl Into<String>, data: Option<T>) -> ActionExecutionResult<T> {
    ActionExecutionResult {
        result_type: ActionResultType::Success,
        message: message.into(),
        data,
        fallback_used: false,
    }
}

#[allow(dead_code)]
pub fn action_info<T>(message: impl Into<String>, data: Option<T>) -> ActionExecutionResult<T> {
    ActionExecutionResult {
        result_type: ActionResultType::Info,
        message: message.into(),
        data,
        fallback_used: false,
    }
}

pub fn action_error<T>(message: impl Into<String>) -> ActionExecutionResult<T> {
    ActionExecutionResult {
        result_type: ActionResultType::Error,
        message: message.into(),
        data: None,
        fallback_used: false,
    }
}

#[cfg(test)]
mod tests {
    use super::{action_error, action_info, action_success, ActionResultType};

    #[test]
    fn constructors_set_result_type() {
        let success = action_success("ok", Some(1usize));
        assert_eq!(success.result_type, ActionResultType::Success);

        let info = action_info::<usize>("note", None);
        assert_eq!(info.result_type, ActionResultType::Info);

        let err = action_error::<usize>("fail");
        assert_eq!(err.result_type, ActionResultType::Error);
    }
}
