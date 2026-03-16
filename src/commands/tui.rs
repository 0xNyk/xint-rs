use anyhow::Result;

use crate::cli::{PolicyMode, TuiArgs};

pub async fn run(_args: &TuiArgs, policy: PolicyMode) -> Result<()> {
    crate::tui::run(policy).await
}
