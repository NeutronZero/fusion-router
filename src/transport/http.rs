use async_trait::async_trait;
use reqwest::Client;
use std::time::Duration;
use crate::providers::{Transport, TransportRequest, TransportResponse, TransportEvent, TransportError};
use futures::StreamExt;

pub struct HttpTransport {
    client: Client,
}

impl HttpTransport {
    pub fn new(timeout: Duration) -> Self {
        Self {
            client: Client::builder()
                .timeout(timeout)
                .build()
                .unwrap_or_default(),
        }
    }
}

impl Default for HttpTransport {
    fn default() -> Self {
        Self::new(Duration::from_secs(30))
    }
}

#[async_trait]
impl Transport for HttpTransport {
    async fn send(&self, req: TransportRequest) -> Result<TransportResponse, TransportError> {
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
        
        let body = resp
            .json::<serde_json::Value>()
            .await
            .map_err(|e| TransportError::Serialization(e.to_string()))?;
            
        Ok(TransportResponse { status, body })
    }

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
