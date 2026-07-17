use async_trait::async_trait;
use serde_json::Value;

use super::Tool;

pub struct HTTPRequestTool {
    client: reqwest::Client,
}

impl HTTPRequestTool {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .expect("Failed to create HTTP client"),
        }
    }
}

impl Default for HTTPRequestTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for HTTPRequestTool {
    fn name(&self) -> &str {
        "http_request"
    }

    fn description(&self) -> &str {
        "Makes HTTP requests to external URLs. Supports GET, POST, PUT, DELETE methods."
    }

    fn schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "method": {
                    "type": "string",
                    "enum": ["GET", "POST", "PUT", "DELETE"],
                    "description": "HTTP method"
                },
                "url": {
                    "type": "string",
                    "description": "Request URL"
                },
                "headers": {
                    "type": "object",
                    "description": "Optional request headers"
                },
                "body": {
                    "type": "object",
                    "description": "Optional request body (for POST/PUT)"
                }
            },
            "required": ["method", "url"]
        })
    }

    async fn execute(&self, args: Value) -> Result<Value, String> {
        let method = args.get("method")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "Missing 'method' argument".to_string())?;

        let url = args.get("url")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "Missing 'url' argument".to_string())?;

        let headers = args.get("headers").and_then(|v| v.as_object());

        let mut request = match method {
            "GET" => self.client.get(url),
            "POST" => {
                let body = args.get("body").cloned().unwrap_or(Value::Null);
                self.client.post(url).json(&body)
            }
            "PUT" => {
                let body = args.get("body").cloned().unwrap_or(Value::Null);
                self.client.put(url).json(&body)
            }
            "DELETE" => self.client.delete(url),
            _ => return Err(format!("Unsupported HTTP method: {}", method)),
        };

        if let Some(hdrs) = headers {
            for (key, value) in hdrs {
                if let Some(val_str) = value.as_str() {
                    request = request.header(key.as_str(), val_str);
                }
            }
        }

        let response = request.send().await
            .map_err(|e| format!("HTTP request failed: {}", e))?;

        let status = response.status().as_u16();
        let body: Value = response.json().await
            .unwrap_or(Value::Null);

        Ok(serde_json::json!({
            "status": status,
            "body": body
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_http_tool_invalid_url() {
        let tool = HTTPRequestTool::new();
        let result = tool.execute(serde_json::json!({
            "method": "GET",
            "url": "not-a-valid-url"
        })).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_http_tool_missing_args() {
        let tool = HTTPRequestTool::new();
        let result = tool.execute(serde_json::json!({})).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("'method'"));
    }

    #[tokio::test]
    async fn test_http_tool_unsupported_method() {
        let tool = HTTPRequestTool::new();
        let result = tool.execute(serde_json::json!({
            "method": "PATCH",
            "url": "https://example.com"
        })).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Unsupported"));
    }
}
