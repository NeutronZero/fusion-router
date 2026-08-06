use async_trait::async_trait;
use reqwest::Client;
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
    pub fn new(timeout: Duration) -> Self {
        Self {
            client: Client::builder()
                .timeout(timeout)
                .build()
                .unwrap_or_default(),
            backoff_base_ms: DEFAULT_BACKOFF_BASE_MS,
            backoff_max_ms: DEFAULT_BACKOFF_MAX_MS,
            max_retries: DEFAULT_MAX_RETRIES,
        }
    }

    pub fn with_backoff(timeout: Duration, base_ms: u64, max_ms: u64, max_retries: u32) -> Self {
        Self {
            client: Client::builder()
                .timeout(timeout)
                .build()
                .unwrap_or_default(),
            backoff_base_ms: base_ms,
            backoff_max_ms: max_ms,
            max_retries,
        }
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
        Self::new(Duration::from_secs(30))
    }
}

#[async_trait]
impl Transport for HttpTransport {
    #[tracing::instrument(skip(self, req), fields(url = %req.url, method = %req.method))]
    async fn send(&self, req: TransportRequest) -> Result<TransportResponse, TransportError> {
        let mut backoff = Backoff::new(self.backoff_base_ms, self.backoff_max_ms);

        for attempt in 0..=self.max_retries {
            let result = self.send_once(&req).await;
            match result {
                Ok(response) => {
                    backoff.reset();
                    if response.status == 429 && attempt < self.max_retries {
                        tokio::time::sleep(backoff.next()).await;
                        continue;
                    }
                    return Ok(response);
                }
                Err(e) => {
                    if attempt == self.max_retries {
                        return Err(e);
                    }
                    tokio::time::sleep(backoff.next()).await;
                }
            }
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
        
        let stream = resp.bytes_stream().map(|chunk_res| {
            match chunk_res {
                Ok(bytes) => {
                    let data = String::from_utf8_lossy(&bytes).to_string();
                    Ok(TransportEvent { data })
                }
                Err(e) => Err(TransportError::Network(e.to_string()))
            }
        });
        
        Ok(Box::pin(stream))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_with_backoff_stores_settings() {
        let transport = HttpTransport::with_backoff(Duration::from_secs(5), 250, 4_000, 3);

        assert_eq!(transport.backoff_base_ms, 250);
        assert_eq!(transport.backoff_max_ms, 4_000);
        assert_eq!(transport.max_retries, 3);
    }

    #[test]
    fn test_new_uses_default_backoff_settings() {
        let transport = HttpTransport::new(Duration::from_secs(30));

        assert_eq!(transport.backoff_base_ms, DEFAULT_BACKOFF_BASE_MS);
        assert_eq!(transport.backoff_max_ms, DEFAULT_BACKOFF_MAX_MS);
        assert_eq!(transport.max_retries, DEFAULT_MAX_RETRIES);
    }
}
