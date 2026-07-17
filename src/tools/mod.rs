mod builtin;
mod http_tool;
mod registry;

pub use http_tool::HTTPRequestTool;
pub use registry::ToolRegistry;

use async_trait::async_trait;
use serde_json::Value;

#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn schema(&self) -> Value;
    async fn execute(&self, args: Value) -> Result<Value, String>;
}
