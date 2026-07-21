use async_trait::async_trait;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;

use crate::transport::{Transport, TransportRequest, TransportResponse, TransportEvent, TransportError};

pub struct StdioTransport {
    command: String,
    args: Vec<String>,
}

impl StdioTransport {
    pub fn new(command: String, args: Vec<String>) -> Self {
        Self { command, args }
    }
}

#[async_trait]
impl Transport for StdioTransport {
    #[tracing::instrument(skip(self, req), fields(command = %self.command))]
    async fn send(&self, req: TransportRequest) -> Result<TransportResponse, TransportError> {
        let mut child = Command::new(&self.command)
            .args(&self.args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| TransportError::Network(format!("Stdio spawn error: {}", e)))?;

        let stdin = child
            .stdin
            .as_mut()
            .ok_or_else(|| TransportError::Network("Failed to open stdin".to_string()))?;

        let input = serde_json::to_string(&req.body)
            .map_err(|e| TransportError::Serialization(e.to_string()))?;

        stdin
            .write_all(input.as_bytes())
            .await
            .map_err(|e| TransportError::Network(format!("Stdio write error: {}", e)))?;

        stdin
            .flush()
            .await
            .map_err(|e| TransportError::Network(format!("Stdio flush error: {}", e)))?;

        let stdout = child
            .stdout
            .as_mut()
            .ok_or_else(|| TransportError::Network("Failed to open stdout".to_string()))?;

        let mut reader = BufReader::new(stdout);
        let mut response = String::new();
        reader
            .read_line(&mut response)
            .await
            .map_err(|e| TransportError::Network(format!("Stdio read error: {}", e)))?;

        Ok(TransportResponse {
            status: 200,
            body: serde_json::Value::String(response.trim().to_string()),
        })
    }

    #[tracing::instrument(skip(self, _req))]
    async fn stream(&self, _req: TransportRequest) -> Result<futures::stream::BoxStream<'static, Result<TransportEvent, TransportError>>, TransportError> {
        Err(TransportError::Network("Streaming not yet supported for stdio transport".to_string()))
    }
}
