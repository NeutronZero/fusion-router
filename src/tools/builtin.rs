use async_trait::async_trait;
use serde_json::Value;

use super::Tool;

pub struct CalculatorTool;

#[async_trait]
impl Tool for CalculatorTool {
    fn name(&self) -> &str {
        "calculator"
    }

    fn description(&self) -> &str {
        "Evaluates arithmetic expressions"
    }

    fn schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "expression": {
                    "type": "string",
                    "description": "Arithmetic expression to evaluate (e.g., 2 + 2)"
                }
            },
            "required": ["expression"]
        })
    }

    async fn execute(&self, args: Value) -> Result<Value, String> {
        let expr = args.get("expression")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "Missing 'expression' argument".to_string())?;

        let result = meval::eval_str(expr)
            .map_err(|e| format!("Calculation error: {}", e))?;

        Ok(serde_json::json!({ "result": result }))
    }
}

pub struct SearchTool;

#[async_trait]
impl Tool for SearchTool {
    fn name(&self) -> &str {
        "search"
    }

    fn description(&self) -> &str {
        "Searches the web for information (mocked)"
    }

    fn schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Search query"
                }
            },
            "required": ["query"]
        })
    }

    async fn execute(&self, args: Value) -> Result<Value, String> {
        let query = args.get("query")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "Missing 'query' argument".to_string())?;

        Ok(serde_json::json!({
            "result": format!("Mock search results for: {}", query)
        }))
    }
}

pub struct FileReadTool {
    allowed_dir: String,
}

impl FileReadTool {
    pub fn new(allowed_dir: String) -> Self {
        Self { allowed_dir }
    }
}

#[async_trait]
impl Tool for FileReadTool {
    fn name(&self) -> &str {
        "file_read"
    }

    fn description(&self) -> &str {
        "Reads a file from the configured directory"
    }

    fn schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "File path relative to allowed directory"
                }
            },
            "required": ["path"]
        })
    }

    async fn execute(&self, args: Value) -> Result<Value, String> {
        let path = args.get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "Missing 'path' argument".to_string())?;

        let allowed = std::path::Path::new(&self.allowed_dir);
        let full_path = allowed.join(path);
        let canonical = std::fs::canonicalize(&full_path)
            .map_err(|_| "Path does not exist or is inaccessible".to_string())?;
        let allowed_canonical = std::fs::canonicalize(allowed)
            .map_err(|_| "Allowed directory not found".to_string())?;
        if !canonical.starts_with(&allowed_canonical) {
            return Err("Path traversal detected".to_string());
        }

        let content = std::fs::read_to_string(&full_path)
            .map_err(|e| format!("File read error: {}", e))?;

        Ok(serde_json::json!({ "content": content }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_calculator_tool() {
        let tool = CalculatorTool;
        let result = tool.execute(serde_json::json!({"expression": "2 + 3"})).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap()["result"], 5.0);
    }

    #[tokio::test]
    async fn test_calculator_tool_invalid_expression() {
        let tool = CalculatorTool;
        let result = tool.execute(serde_json::json!({"expression": "invalid"})).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_search_tool_mocked() {
        let tool = SearchTool;
        let result = tool.execute(serde_json::json!({"query": "hello"})).await;
        assert!(result.is_ok());
        let val = result.unwrap();
        assert!(val["result"].as_str().unwrap().contains("hello"));
    }

    #[tokio::test]
    async fn test_file_read_tool_path_traversal_blocked() {
        let tmp = std::env::temp_dir();
        let tool = FileReadTool::new(tmp.to_string_lossy().to_string());
        let result = tool.execute(serde_json::json!({"path": "../../etc/passwd"})).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.contains("Path traversal") || err.contains("not found") || err.contains("inaccessible"));
    }
}
