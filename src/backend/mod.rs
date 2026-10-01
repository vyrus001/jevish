pub mod cdp;
pub mod process;

use crate::contracts::{ActionPlan, ExecutionResult, RawSnapshot};
use anyhow::Result;

pub trait BrowserBackend {
    fn snapshot(&mut self) -> Result<RawSnapshot>;
    fn execute(&mut self, plan: &ActionPlan) -> Result<ExecutionResult>;
}
