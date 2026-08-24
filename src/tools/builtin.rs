use async_trait::async_trait;
use serde_json::Value;
use tokio::io::AsyncReadExt;

use super::Tool;

/// Hard cap on file bytes returned by `file_read`. A tool caller (or a
/// prompt-injected file) cannot force the server to slurp arbitrarily large
/// files into memory.
const MAX_FILE_READ_BYTES: usize = 2 * 1024 * 1024;

const MAX_EXPRESSION_LEN: usize = 512;

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
        let expr = args
            .get("expression")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "Missing 'expression' argument".to_string())?;

        if expr.len() > MAX_EXPRESSION_LEN {
            return Err(format!(
                "expression exceeds max length of {MAX_EXPRESSION_LEN} characters"
            ));
        }

        let result = fasteval::ez_eval(expr, &mut fasteval::EmptyNamespace)
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
        let query = args
            .get("query")
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
        let path = args
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "Missing 'path' argument".to_string())?;

        let allowed = std::path::PathBuf::from(&self.allowed_dir);
        let full_path = allowed.join(path);

        let allowed_canonical = tokio::fs::canonicalize(&allowed)
            .await
            .map_err(|_| "Allowed directory not found".to_string())?;
        let canonical = tokio::fs::canonicalize(&full_path)
            .await
            .map_err(|_| "Path does not exist or is inaccessible".to_string())?;
        if !canonical.starts_with(&allowed_canonical) {
            return Err("Path traversal detected".to_string());
        }

        let file = tokio::fs::File::open(&canonical)
            .await
            .map_err(|e| format!("File read error: {}", e))?;
        let mut reader = tokio::io::BufReader::new(file).take((MAX_FILE_READ_BYTES + 1) as u64);
        let mut content = String::new();
        let bytes_read = reader
            .read_to_string(&mut content)
            .await
            .map_err(|e| format!("File read error: {}", e))?;
        let truncated = bytes_read > MAX_FILE_READ_BYTES;
        if truncated {
            content = content
                .get(..MAX_FILE_READ_BYTES)
                .unwrap_or(&content)
                .to_string();
        }

        Ok(serde_json::json!({
            "content": content,
            "bytes_read": bytes_read,
            "truncated": truncated,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_calculator_tool() {
        let tool = CalculatorTool;
        let result = tool
            .execute(serde_json::json!({"expression": "2 + 3"}))
            .await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap()["result"], 5.0);
    }

    #[tokio::test]
    async fn test_calculator_tool_invalid_expression() {
        let tool = CalculatorTool;
        let result = tool
            .execute(serde_json::json!({"expression": "invalid"}))
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_calculator_tool_rejects_oversized_expression() {
        let tool = CalculatorTool;
        let long_expr = "1 + ".repeat(200);
        let result = tool
            .execute(serde_json::json!({"expression": long_expr}))
            .await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("max length"));
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
        let result = tool
            .execute(serde_json::json!({"path": "../../etc/passwd"}))
            .await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.contains("Path traversal")
                || err.contains("not found")
                || err.contains("inaccessible")
        );
    }

    #[tokio::test]
    async fn test_file_read_tool_rejects_absolute_path_outside_allowed_dir() {
        let tmp = std::env::temp_dir();
        let tool = FileReadTool::new(tmp.to_string_lossy().to_string());

        // An absolute path must never resolve relative to the allowed directory
        // (Path::join splices absolute paths, replacing the base).
        let result = tool
            .execute(serde_json::json!({"path": "C:\\Windows\\win.ini"}))
            .await;
        assert!(result.is_err());

        let result = tool
            .execute(serde_json::json!({"path": "/etc/passwd"}))
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_file_read_tool_reads_file_content() {
        let tmp = std::env::temp_dir();
        let unique_name = format!("_fusion_test_readable_{}.txt", uuid::Uuid::new_v4());
        let test_path = tmp.join(&unique_name);
        let test_content = "hello from fusion test";
        std::fs::write(&test_path, test_content).unwrap();

        let tool = FileReadTool::new(tmp.to_string_lossy().to_string());
        let result = tool.execute(serde_json::json!({"path": unique_name})).await;

        let _ = std::fs::remove_file(&test_path);

        assert!(
            result.is_ok(),
            "File read should succeed: {:?}",
            result.err()
        );
        let val = result.unwrap();
        assert_eq!(val["content"].as_str().unwrap(), test_content);
    }

    #[tokio::test]
    async fn test_file_read_tool_truncates_oversized_files() {
        let tmp = std::env::temp_dir();
        let unique_name = format!("_fusion_test_oversize_{}.txt", uuid::Uuid::new_v4());
        let test_path = tmp.join(&unique_name);
        let big_content = "x".repeat(MAX_FILE_READ_BYTES + 1024);
        std::fs::write(&test_path, &big_content).unwrap();

        let tool = FileReadTool::new(tmp.to_string_lossy().to_string());
        let result = tool.execute(serde_json::json!({"path": unique_name})).await;
        let _ = std::fs::remove_file(&test_path);

        let val = result.expect("oversized file must still read, truncated");
        assert_eq!(val["truncated"], serde_json::json!(true));
        assert!(val["content"].as_str().unwrap().len() <= MAX_FILE_READ_BYTES);
        assert!(val["bytes_read"].as_u64().unwrap() > MAX_FILE_READ_BYTES as u64);
    }

    #[tokio::test]
    async fn test_file_read_tool_does_not_block_runtime_under_load() {
        let tmp = std::env::temp_dir();
        let unique_name = format!("_fusion_test_concurrent_{}.txt", uuid::Uuid::new_v4());
        let test_path = tmp.join(&unique_name);
        let test_content = "concurrent test data";
        std::fs::write(&test_path, test_content).unwrap();

        let tool = std::sync::Arc::new(FileReadTool::new(tmp.to_string_lossy().to_string()));
        let mut handles = Vec::new();

        for i in 0..20 {
            let tool = tool.clone();
            let path = unique_name.clone();
            handles.push(tokio::spawn(async move {
                let result = tool.execute(serde_json::json!({"path": path})).await;
                (i, result)
            }));
        }

        let mut ok_count = 0u32;
        for h in handles {
            match h.await {
                Ok((_i, Ok(_))) => ok_count += 1,
                Ok((i, Err(e))) => panic!("Task {} failed: {:?}", i, e),
                Err(e) => panic!("Task join error: {:?}", e),
            }
        }

        let _ = std::fs::remove_file(&test_path);
        assert_eq!(ok_count, 20, "All 20 concurrent file reads should succeed");
    }
}
