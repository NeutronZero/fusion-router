use crate::transport::backoff::Backoff;
use crate::transport::{
    Transport, TransportError, TransportEvent, TransportRequest, TransportResponse,
};
use async_trait::async_trait;
use futures::StreamExt;
use reqwest::Client;
use std::sync::{Arc, Mutex};
use std::time::Duration;

const DEFAULT_MAX_RETRIES: u32 = 5;
const DEFAULT_BACKOFF_BASE_MS: u64 = 1000;
const DEFAULT_BACKOFF_MAX_MS: u64 = 60_000;

/// Hardened default client timeout (policy: never ship a timeout-less client).
const DEFAULT_HTTP_TIMEOUT: Duration = Duration::from_secs(30);
/// Upper bound for buffered upstream error bodies. Provider error payloads
/// are diagnostics, not data; buffering them unboundedly lets a hostile or
/// broken upstream exhaust memory.
pub const MAX_ERROR_BODY_BYTES: usize = 1024 * 1024;
const ERROR_BODY_TRUNCATION_SUFFIX: &str = "\n...[truncated]";

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
        let client = Client::builder()
            .timeout(timeout)
            .build()
            .map_err(|e| TransportError::Network(format!("failed to build HTTP client: {e}")))?;
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
        let client = Client::builder()
            .timeout(timeout)
            .build()
            .map_err(|e| TransportError::Network(format!("failed to build HTTP client: {e}")))?;
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
            let err_body = read_body_capped(resp).await;
            return Err(TransportError::Http {
                status,
                body: err_body,
            });
        }

        let body = resp
            .json::<serde_json::Value>()
            .await
            .map_err(|e| TransportError::Serialization(e.to_string()))?;

        Ok(TransportResponse { status, body })
    }
}

/// Reads an upstream error body with a hard byte cap. Bodies larger than
/// [`MAX_ERROR_BODY_BYTES`] are truncated and marked, never buffered whole.
async fn read_body_capped(mut resp: reqwest::Response) -> String {
    let mut buf: Vec<u8> = Vec::new();
    let mut truncated = false;
    while !truncated {
        match resp.chunk().await {
            Ok(Some(chunk)) => {
                let remaining = MAX_ERROR_BODY_BYTES.saturating_sub(buf.len());
                if chunk.len() > remaining {
                    buf.extend_from_slice(&chunk[..remaining]);
                    truncated = true;
                } else {
                    buf.extend_from_slice(&chunk);
                }
            }
            Ok(None) => break,
            Err(_) => {
                // A partial error body is still usable diagnostics; keep what
                // was read instead of failing the classification.
                break;
            }
        }
    }
    finalize_error_body(buf, truncated)
}

/// Pure helper: lossy-decode a capped byte buffer and mark truncation.
fn finalize_error_body(buf: Vec<u8>, truncated: bool) -> String {
    let mut text = String::from_utf8_lossy(&buf).into_owned();
    if truncated {
        text.push_str(ERROR_BODY_TRUNCATION_SUFFIX);
    }
    text
}

impl Default for HttpTransport {
    /// Fail-fast: silently falling back to `Client::new()` (no timeout)
    /// contradicts the transport hardening policy. If the hardened client
    /// cannot be built, panic with a clear startup-time message instead of
    /// running unbounded requests.
    fn default() -> Self {
        Self::new(DEFAULT_HTTP_TIMEOUT).unwrap_or_else(|e| {
            panic!(
                "HttpTransport::default failed to build hardened HTTP client (timeout {:?}): {e}. \
                 Refusing to start with an unbounded (timeout-less) HTTP client.",
                DEFAULT_HTTP_TIMEOUT
            )
        })
    }
}

/// Retry classification. Timeouts are deliberately NOT retryable: retrying a
/// timed-out provider call re-bills the full (potentially expensive) upstream
/// request and doubles latency on long completions.
fn is_retryable(result: &Result<TransportResponse, TransportError>) -> bool {
    match result {
        Ok(response) => response.status == 429,
        Err(TransportError::Http { status, .. }) => *status == 429 || *status >= 500,
        Err(TransportError::Timeout(_)) => false,
        Err(TransportError::Network(_)) | Err(TransportError::Serialization(_)) => true,
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
        // potentially tripping provider rate limits. Timeouts are never
        // retried (re-billed long calls).
        for attempt in 0..=self.max_retries {
            let result = self.send_once(&req).await;
            if !is_retryable(&result) || attempt == self.max_retries {
                return result;
            }
            tokio::time::sleep(backoff.next()).await;
        }

        Err(TransportError::Network("max retries exceeded".to_string()))
    }

    #[tracing::instrument(skip(self, req), fields(url = %req.url, method = %req.method))]
    async fn stream(
        &self,
        req: TransportRequest,
    ) -> Result<
        futures::stream::BoxStream<'static, Result<TransportEvent, TransportError>>,
        TransportError,
    > {
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
            let err_body = read_body_capped(resp).await;
            return Err(TransportError::Http {
                status,
                body: err_body,
            });
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
                        Ok(TransportEvent {
                            data: drain_utf8(&mut buf),
                        })
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
                let text = String::from_utf8_lossy(&carry[..valid + err_len]).into_owned();
                carry.drain(..valid + err_len);
                text
            }
            None => {
                let valid = e.valid_up_to();
                if valid > 0 {
                    let text = String::from_utf8_lossy(&carry[..valid]).into_owned();
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
        let transport = HttpTransport::with_backoff(Duration::from_secs(5), 250, 4_000, 3).unwrap();

        assert_eq!(transport.backoff_base_ms, 250);
        assert_eq!(transport.backoff_max_ms, 4_000);
        assert_eq!(transport.max_retries, 3);
    }

    #[test]
    fn test_timeout_is_not_retryable() {
        let timeout: Result<TransportResponse, TransportError> =
            Err(TransportError::Timeout("request timed out".into()));
        assert!(
            !is_retryable(&timeout),
            "retrying a timed-out provider call re-bills the full request; must not retry"
        );
    }

    #[test]
    fn test_retryable_classification_table() {
        let ok_429 = Ok(TransportResponse {
            status: 429,
            body: serde_json::json!({}),
        });
        let ok_200 = Ok(TransportResponse {
            status: 200,
            body: serde_json::json!({}),
        });
        let http_500: Result<TransportResponse, TransportError> = Err(TransportError::Http {
            status: 500,
            body: String::new(),
        });
        let http_400: Result<TransportResponse, TransportError> = Err(TransportError::Http {
            status: 400,
            body: String::new(),
        });
        let network: Result<TransportResponse, TransportError> =
            Err(TransportError::Network("conn reset".into()));
        let serialization: Result<TransportResponse, TransportError> =
            Err(TransportError::Serialization("bad json".into()));

        assert!(is_retryable(&ok_429), "rate limit is retryable");
        assert!(!is_retryable(&ok_200));
        assert!(is_retryable(&http_500), "server error is retryable");
        assert!(!is_retryable(&http_400), "client error fails fast");
        assert!(is_retryable(&network));
        assert!(is_retryable(&serialization));
    }

    #[test]
    fn test_finalize_error_body_marks_truncation() {
        let full = finalize_error_body(b"error detail".to_vec(), false);
        assert_eq!(full, "error detail");
        assert!(!full.contains("truncated"));

        let truncated = finalize_error_body(vec![b'x'; MAX_ERROR_BODY_BYTES], true);
        assert!(truncated.contains(ERROR_BODY_TRUNCATION_SUFFIX));
        assert!(truncated.len() < MAX_ERROR_BODY_BYTES + ERROR_BODY_TRUNCATION_SUFFIX.len() + 16);
    }

    #[test]
    fn test_finalize_error_body_lossy_on_invalid_utf8() {
        let mut buf = b"partial ".to_vec();
        buf.push(0xFF); // invalid byte
        let text = finalize_error_body(buf, false);
        assert_eq!(text, "partial \u{FFFD}");
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
        assert_eq!(
            drain_utf8(&mut carry),
            "",
            "incomplete sequence stays buffered"
        );
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
