use async_trait::async_trait;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};

use super::{Transport, TransportStream};

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
    async fn send(&self, request: &str) -> Result<String, String> {
        let mut child = Command::new(&self.command)
            .args(&self.args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| format!("Stdio spawn error: {}", e))?;

        let stdin = child
            .stdin
            .as_mut()
            .ok_or_else(|| "Failed to open stdin".to_string())?;

        stdin
            .write_all(request.as_bytes())
            .await
            .map_err(|e| format!("Stdio write error: {}", e))?;

        stdin
            .flush()
            .await
            .map_err(|e| format!("Stdio flush error: {}", e))?;

        let stdout = child
            .stdout
            .as_mut()
            .ok_or_else(|| "Failed to open stdout".to_string())?;

        let mut reader = BufReader::new(stdout);
        let mut response = String::new();
        reader
            .read_line(&mut response)
            .await
            .map_err(|e| format!("Stdio read error: {}", e))?;

        Ok(response.trim().to_string())
    }

    async fn stream(&self, _request: &str) -> Result<Box<dyn TransportStream>, String> {
        Err("Streaming not yet supported for stdio transport".to_string())
    }
}

pub struct StdioTransportStream {
    child: Child,
}

#[async_trait]
impl TransportStream for StdioTransportStream {
    async fn next(&mut self) -> Option<String> {
        let stdout = self.child.stdout.as_mut()?;
        let mut reader = BufReader::new(stdout);
        let mut line = String::new();
        reader.read_line(&mut line).await.ok()?;
        if line.is_empty() {
            None
        } else {
            Some(line.trim().to_string())
        }
    }
}
