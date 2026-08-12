use async_trait::async_trait;
use reqwest::Client;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use crate::transport::backoff::Backoff;
use crate::transport::{Transport, TransportRequest, TransportResponse, TransportEvent, TransportError};
use futures::StreamExt;

const DEFAULT_MAX_RETRIES: u32 = 5;
const DEFAULT_BACKOFF_BASE_MS: u64 = 1000;
const DEFAULT_BACKOFF_MAX_MS: u64 = 60_000;

pub struct HttpTransport {
    client: Client,
    backoff_base_ms: u64,
    backoff_max_ms: u64,
    max_retries: u32,
}

impl HttpTransport {
    /// Builds a transport with the configured request timeout. Client
    /// construction failures propagate instead of being replaced by a default
    /// (timeout-less) client, so a misconfigured TLS/proxy setup surfaces at
    /// startup rather than producing unbounded requests at runtime.
    pub fn new(timeout: Duration) -> Result<Self, TransportError> {
        let client = Client::builder().timeout(timeout).build().map_err(|e| {
            TransportError::Network(format!("failed to build HTTP client: {e}"))
        })?;
        Ok(Self {
            client,
            backoff_base_ms: DEFAULT_BACKOFF_BASE_MS,
            backoff_max_ms: DEFAULT_BACKOFF_MAX_MS,
            max_retries: DEFAULT_MAX_RETRIES,
        })
    }

    pub fn with_backoff(
        timeout: Duration,
        base_ms: u64,
        max_ms: u64,
        max_retries: u32,
    ) -> Result<Self, TransportError> {
        let client = Client::builder().timeout(timeout).build().map_err(|e| {
            TransportError::Network(format!("failed to build HTTP client: {e}"))
        })?;
        Ok(Self {
            client,
            backoff_base_ms: base_ms,
            backoff_max_ms: max_ms,
            max_retries,
        })
    }

    async fn send_once(&self, req: &TransportRequest) -> Result<TransportResponse, TransportError> {
        let mut request = match req.method.as_str() {
            "GET" => self.client.get(&req.url),
            _ => self.client.post(&req.url),
        };

        for (k, v) in &req.headers {
            request = request.header(k, v);
        }

        let resp = request
            .json(&req.body)
            .send()
            .await
            .map_err(|e| TransportError::Network(e.to_string()))?;

        let status = resp.status().as_u16();
        if status >= 400 {
            let err_body = resp.text().await.unwrap_or_default();
            return Err(TransportError::Http { status, body: err_body });
        }

        let body = resp
            .json::<serde_json::Value>()
            .await
            .map_err(|e| TransportError::Serialization(e.to_string()))?;

        Ok(TransportResponse { status, body })
    }
}

impl Default for HttpTransport {
    fn default() -> Self {
        Self::new(Duration::from_secs(30)).unwrap_or_else(|e| {
            tracing::error!(error = %e, "failed to build configured HTTP client for transport; falling back to default Client");
            Self {
                client: Client::new(),
                backoff_base_ms: DEFAULT_BACKOFF_BASE_MS,
                backoff_max_ms: DEFAULT_BACKOFF_MAX_MS,
                max_retries: DEFAULT_MAX_RETRIES,
            }
        })
    }
}

#[async_trait]
impl Transport for HttpTransport {
    #[tracing::instrument(skip(self, req), fields(url = %req.url, method = %req.method))]
    async fn send(&self, req: TransportRequest) -> Result<TransportResponse, TransportError> {
        let mut backoff = Backoff::new(self.backoff_base_ms, self.backoff_max_ms);

        // Only transient failures are retried: rate limits (429), server
        // errors (5xx), and network/serialization hiccups. Permanent client
        // errors (4xx) fail immediately instead of wasting latency and
        // potentially tripping provider rate limits.
        for attempt in 0..=self.max_retries {
            let result = self.send_once(&req).await;
            let should_retry = match &result {
                Ok(response) => response.status == 429,
                Err(TransportError::Http { status, .. }) => *status == 429 || *status >= 500,
                Err(TransportError::Network(_))
                | Err(TransportError::Timeout(_))
                | Err(TransportError::Serialization(_)) => true,
            };
            if !should_retry || attempt == self.max_retries {
                return result;
            }
            tokio::time::sleep(backoff.next()).await;
        }

        Err(TransportError::Network("max retries exceeded".to_string()))
    }

    #[tracing::instrument(skip(self, req), fields(url = %req.url, method = %req.method))]
    async fn stream(&self, req: TransportRequest) -> Result<futures::stream::BoxStream<'static, Result<TransportEvent, TransportError>>, TransportError> {
        let mut request = match req.method.as_str() {
            "GET" => self.client.get(&req.url),
            _ => self.client.post(&req.url),
        };

        for (k, v) in req.headers {
            request = request.header(k, v);
        }

        let resp = request
            .json(&req.body)
            .send()
            .await
            .map_err(|e| TransportError::Network(e.to_string()))?;

        let status = resp.status().as_u16();
        if status >= 400 {
            let err_body = resp.text().await.unwrap_or_default();
            return Err(TransportError::Http { status, body: err_body });
        }

        // Chunk boundaries are arbitrary byte counts: a multi-byte UTF-8
        // sequence may be split across two chunks. `drain_utf8` decodes only
        // complete sequences per chunk and carries any trailing partial
        // sequence into the next chunk, so streamed text is never corrupted
        // by from_utf8_lossy() mangling a split character mid-stream.
        let pending: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));
        let stream = resp.bytes_stream().map({
            let pending = pending.clone();
            move |chunk_res| -> Result<TransportEvent, TransportError> {
                match chunk_res {
                    Ok(bytes) => {
                        let mut buf = pending.lock().unwrap_or_else(|e| e.into_inner());
                        buf.extend_from_slice(&bytes);
                        Ok(TransportEvent { data: drain_utf8(&mut buf) })
                    }
                    Err(e) => Err(TransportError::Network(e.to_string())),
                }
            }
        });

        // Flush any bytes still held back at end of stream: a trailing
        // partial multi-byte sequence is decoded lossily instead of dropped.
        let tail = pending;
        let flushed = futures::stream::once(async move {
            let buf = tail.lock().unwrap_or_else(|e| e.into_inner()).clone();
            if buf.is_empty() {
                None
            } else {
                Some(Ok(TransportEvent {
                    data: String::from_utf8_lossy(&buf).into_owned(),
                }))
            }
        })
        .filter_map(|flush| async move { flush });

        Ok(Box::pin(stream.chain(flushed)))
    }
}

/// Appends `bytes` to `carry` and returns the complete UTF-8 text decodable
/// so far. Any trailing partial multi-byte sequence is left buffered for the
/// next call; only truly invalid bytes are replaced (U+FFFD), never valid
/// bytes misaligned by a chunk split.
fn drain_utf8(carry: &mut Vec<u8>) -> String {
    if carry.is_empty() {
        return String::new();
    }
    match std::str::from_utf8(carry) {
        Ok(s) => {
            let text = s.to_string();
            carry.clear();
            text
        }
        Err(e) => match e.error_len() {
            Some(err_len) => {
                let valid = e.valid_up_to();
                let text =
                    String::from_utf8_lossy(&carry[..valid + err_len]).into_owned();
                carry.drain(..valid + err_len);
                text
            }
            None => {
                let valid = e.valid_up_to();
                if valid > 0 {
                    let text = std::str::from_utf8(&carry[..valid])
                        .expect("valid_up_to bytes are valid UTF-8")
                        .to_string();
                    carry.drain(..valid);
                    text
                } else {
                    String::new()
                }
            }
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_with_backoff_stores_settings() {
        let transport =
            HttpTransport::with_backoff(Duration::from_secs(5), 250, 4_000, 3).unwrap();

        assert_eq!(transport.backoff_base_ms, 250);
        assert_eq!(transport.backoff_max_ms, 4_000);
        assert_eq!(transport.max_retries, 3);
    }

    #[test]
    fn test_new_uses_default_backoff_settings() {
        let transport = HttpTransport::new(Duration::from_secs(30)).unwrap();

        assert_eq!(transport.backoff_base_ms, DEFAULT_BACKOFF_BASE_MS);
        assert_eq!(transport.backoff_max_ms, DEFAULT_BACKOFF_MAX_MS);
        assert_eq!(transport.max_retries, DEFAULT_MAX_RETRIES);
    }

    #[test]
    fn test_drain_utf8_joins_split_multibyte_character() {
        // "中文" encoded as bytes; split mid-character at every boundary.
        let text = "Hello 中文 world 🚀";
        let bytes = text.as_bytes();
        for split in 1..bytes.len() {
            let mut carry: Vec<u8> = Vec::new();
            let mut decoded = String::new();
            decoded.push_str(&drain_utf8(&mut carry));
            for &byte in &bytes[..split] {
                carry.push(byte);
                decoded.push_str(&drain_utf8(&mut carry));
            }
            for &byte in &bytes[split..] {
                carry.push(byte);
                decoded.push_str(&drain_utf8(&mut carry));
            }
            assert_eq!(decoded, text, "split at byte {split} must not corrupt text");
        }
    }

    #[test]
    fn test_drain_utf8_yields_valid_prefix_on_incomplete_tail() {
        let mut carry = Vec::new();
        carry.extend_from_slice("Hello ".as_bytes());
        carry.extend_from_slice(&[0xE4, 0xB8]); // first two bytes of U+4E2D (中)
        let prefix = drain_utf8(&mut carry);
        assert_eq!(prefix, "Hello ", "valid prefix text must yield immediately");
        assert_eq!(carry, vec![0xE4, 0xB8], "incomplete tail stays buffered");

        carry.extend_from_slice(&[0xAD]); // third byte completing 中
        let remainder = drain_utf8(&mut carry);
        assert_eq!(remainder, "中");
        assert!(carry.is_empty());
    }

    #[test]
    fn test_drain_utf8_flushes_partial_tail_lossily() {
        let mut carry = vec![0xE4, 0xB8]; // first two bytes of U+4E2D (中)
        assert_eq!(drain_utf8(&mut carry), "", "incomplete sequence stays buffered");
        // Stream ends: flush decodes lossily instead of dropping.
        assert_eq!(String::from_utf8_lossy(&carry), "\u{FFFD}");
    }

    #[test]
    fn test_drain_utf8_replaces_only_invalid_bytes() {
        let mut carry = "ok ".as_bytes().to_vec();
        carry.push(0xFF); // invalid byte
        carry.extend_from_slice("fine".as_bytes());
        let out = drain_utf8(&mut carry);
        assert_eq!(out, "ok \u{FFFD}");
        // The valid bytes after the invalid one are preserved for the next
        // call instead of being swallowed by the replacement.
        let out = drain_utf8(&mut carry);
        assert_eq!(out, "fine");
        assert!(carry.is_empty());
    }
}
