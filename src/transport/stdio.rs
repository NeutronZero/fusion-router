use async_trait::async_trait;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;

use crate::transport::{
    Transport, TransportError, TransportEvent, TransportRequest, TransportResponse,
};

/// Default bound on how long a single stdio round-trip may take before the
/// child is abandoned. Prevents EOF-waiters from hanging forever on children
/// that never exit.
pub const DEFAULT_READ_TIMEOUT: Duration = Duration::from_secs(30);

pub struct StdioTransport {
    command: String,
    args: Vec<String>,
    read_timeout: Duration,
}

impl StdioTransport {
    pub fn new(command: String, args: Vec<String>) -> Self {
        Self {
            command,
            args,
            read_timeout: DEFAULT_READ_TIMEOUT,
        }
    }

    /// Overrides the per-request read timeout.
    pub fn with_read_timeout(mut self, timeout: Duration) -> Self {
        self.read_timeout = timeout;
        self
    }
}

#[async_trait]
impl Transport for StdioTransport {
    #[tracing::instrument(skip(self, req), fields(command = %self.command))]
    async fn send(&self, req: TransportRequest) -> Result<TransportResponse, TransportError> {
        // kill_on_drop ensures the child cannot outlive the request future:
        // if this call times out or errors, the spawned process is reaped
        // instead of leaking.
        let mut child = Command::new(&self.command)
            .args(&self.args)
            .kill_on_drop(true)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| TransportError::Network(format!("Stdio spawn error: {}", e)))?;

        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| TransportError::Network("Failed to open stdin".to_string()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| TransportError::Network("Failed to open stdout".to_string()))?;

        let input = serde_json::to_string(&req.body)
            .map_err(|e| TransportError::Serialization(e.to_string()))?;

        let write_result = async {
            stdin.write_all(input.as_bytes()).await?;
            stdin.flush().await?;
            // Shut the pipe down so the child sees EOF and cannot leave us
            // waiting for output it will only produce after stdin closes.
            stdin.shutdown().await
        };
        if let Err(e) = write_result.await {
            return Err(TransportError::Network(format!("Stdio write error: {}", e)));
        }
        drop(stdin);

        let mut reader = BufReader::new(stdout);
        let read_fut = async {
            let mut response = String::new();
            reader.read_line(&mut response).await.map(|_| response)
        };
        let response = match tokio::time::timeout(self.read_timeout, read_fut).await {
            Ok(Ok(response)) => response,
            Ok(Err(e)) => {
                return Err(TransportError::Network(format!("Stdio read error: {}", e)));
            }
            Err(_elapsed) => {
                return Err(TransportError::Timeout(format!(
                    "Stdio read timed out after {}s",
                    self.read_timeout.as_secs()
                )));
            }
        };

        Ok(TransportResponse {
            status: 200,
            body: serde_json::Value::String(response.trim().to_string()),
        })
    }

    #[tracing::instrument(skip(self, _req))]
    async fn stream(
        &self,
        _req: TransportRequest,
    ) -> Result<
        futures::stream::BoxStream<'static, Result<TransportEvent, TransportError>>,
        TransportError,
    > {
        Err(TransportError::Network(
            "Streaming not yet supported for stdio transport".to_string(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn sample_request() -> TransportRequest {
        TransportRequest {
            url: String::new(),
            method: "POST".into(),
            headers: HashMap::new(),
            body: serde_json::json!({"ping": true}),
        }
    }

    #[test]
    fn test_new_stores_command_and_args() {
        let transport = StdioTransport::new("python".into(), vec!["-c".into(), "print(1)".into()]);

        assert_eq!(transport.command, "python");
        assert_eq!(
            transport.args,
            vec!["-c".to_string(), "print(1)".to_string()]
        );
        assert_eq!(transport.read_timeout, DEFAULT_READ_TIMEOUT);
    }

    #[test]
    fn test_with_read_timeout_overrides_default() {
        let transport =
            StdioTransport::new("cmd".into(), vec![]).with_read_timeout(Duration::from_millis(250));
        assert_eq!(transport.read_timeout, Duration::from_millis(250));
    }

    #[tokio::test]
    async fn test_send_maps_spawn_failure_to_network_error() {
        let transport = StdioTransport::new(
            "definitely-not-a-real-binary-xyz".into(),
            vec!["--version".into()],
        );
        let err = transport.send(sample_request()).await.unwrap_err();
        assert!(
            matches!(err, TransportError::Network(ref m) if m.contains("spawn")),
            "unexpected error: {err:?}"
        );
    }

    /// A silently-blocking child (never writes stdout) must surface a Timeout
    /// error once the configured deadline elapses, rather than hanging.
    /// Ignored by default: spawns real OS processes and depends on platform
    /// utilities being present; run manually with `--ignored`.
    #[tokio::test]
    #[ignore = "spawns OS processes (waitfor/sleep); kept for manual verification due to CI flakiness"]
    async fn test_send_times_out_on_silent_child() {
        #[cfg(windows)]
        let transport = StdioTransport::new(
            "waitfor".into(),
            vec!["fusion_stdio_never_signal".into(), "/t".into(), "60".into()],
        )
        .with_read_timeout(Duration::from_millis(300));
        #[cfg(not(windows))]
        let transport = StdioTransport::new("sleep".into(), vec!["60".into()])
            .with_read_timeout(Duration::from_millis(300));

        let start = std::time::Instant::now();
        let err = transport.send(sample_request()).await.unwrap_err();
        assert!(
            matches!(err, TransportError::Timeout(_)),
            "expected timeout, got: {err:?}"
        );
        // Must return promptly, not wait out the full child lifetime.
        assert!(start.elapsed() < Duration::from_secs(10));
    }
}
