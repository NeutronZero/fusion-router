use async_trait::async_trait;
use rusqlite::{params, Connection};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tracing::info;

use crate::telemetry::{EvidenceRepository, ModelPerformanceStats};
use crate::types::{EvidenceSnapshot, ExecutionRecord};

pub struct SqliteEvidenceRepository {
    conn: Arc<Mutex<Connection>>,
}

impl SqliteEvidenceRepository {
pub fn new(path: &str) -> anyhow::Result<Self> {
        let conn = Connection::open(path)?;
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA busy_timeout=5000;
             CREATE TABLE IF NOT EXISTS execution_records (
                 record_id TEXT PRIMARY KEY,
                 plan_id TEXT NOT NULL,
                 node_id TEXT NOT NULL,
                 model TEXT NOT NULL,
                 provider TEXT NOT NULL,
                 intent TEXT NOT NULL,
                 latency_ms INTEGER NOT NULL,
                 tokens INTEGER NOT NULL,
                 cost INTEGER NOT NULL,
                 success INTEGER NOT NULL,
                 timestamp INTEGER NOT NULL
             )",
        )?;
        info!("SQLite evidence repository initialized at path: {}", path);
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }
}

#[async_trait]
impl EvidenceRepository for SqliteEvidenceRepository {
    async fn record(&self, entry: ExecutionRecord) -> anyhow::Result<()> {
        let conn = self.conn.clone();
        tokio::task::spawn_blocking(move || {
            let conn = conn.lock().unwrap_or_else(|e| e.into_inner());
            conn.execute(
                "INSERT INTO execution_records
                 (record_id, plan_id, node_id, model, provider, intent, latency_ms, tokens, cost, success, timestamp)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                params![
                    entry.record_id.to_string(),
                    entry.plan_id.to_string(),
                    entry.node_id.to_string(),
                    entry.model,
                    entry.provider,
                    format!("{:?}", entry.intent),
                    entry.latency_ms as i64,
                    entry.tokens as i64,
                    entry.cost.as_nanos() as i64,
                    entry.success as i32,
                    entry.timestamp,
                ],
            )?;
            Ok(())
        })
        .await?
    }

    async fn snapshot(&self) -> anyhow::Result<EvidenceSnapshot> {
        let conn = self.conn.clone();
        tokio::task::spawn_blocking(move || {
            let conn = conn.lock().unwrap_or_else(|e| e.into_inner());
            let tx = conn.unchecked_transaction()?;

            let record_count: u64 = tx
                .query_row("SELECT COUNT(*) FROM execution_records", [], |row| {
                    row.get::<_, i64>(0)
                })? as u64;

            let mut success_rates: HashMap<String, f64> = HashMap::new();
            {
                let mut stmt = tx.prepare(
                    "SELECT model, intent,
                            CAST(SUM(success) AS REAL) / CAST(COUNT(*) AS REAL) AS rate
                     FROM execution_records
                     GROUP BY model, intent",
                )?;
                let rows = stmt.query_map([], |row| {
                    let model: String = row.get(0)?;
                    let intent: String = row.get(1)?;
                    let rate: f64 = row.get(2)?;
                    Ok((model, intent, rate))
                })?;
                for row in rows {
                    let (model, intent, rate) = row?;
                    success_rates.insert(format!("{}::{}", model, intent), rate);
                }
            }

            let mut avg_latencies: HashMap<String, f64> = HashMap::new();
            let mut avg_costs: HashMap<String, crate::types::NanoUSD> = HashMap::new();
            let mut model_rankings: Vec<String> = Vec::new();
            {
                let mut stmt = tx.prepare(
                    "SELECT model,
                            AVG(latency_ms),
                            CAST(AVG(cost) AS INTEGER),
                            CAST(SUM(success) AS REAL) / CAST(COUNT(*) AS REAL) AS rate
                     FROM execution_records
                     GROUP BY model
                     ORDER BY rate DESC",
                )?;
                let rows = stmt.query_map([], |row| {
                    let model: String = row.get(0)?;
                    let avg_lat: f64 = row.get(1)?;
                    let avg_cost_nanos: i64 = row.get(2)?;
                    Ok((model, avg_lat, avg_cost_nanos))
                })?;
                for row in rows {
                    let (model, avg_lat, avg_cost_nanos) = row?;
                    avg_latencies.insert(model.clone(), avg_lat);
                    avg_costs.insert(model.clone(), crate::types::NanoUSD::from_nanos(avg_cost_nanos.max(0) as u64));
                    model_rankings.push(model);
                }
            }

            tx.commit()?;

            Ok(EvidenceSnapshot {
                record_count,
                success_rates,
                avg_latencies,
                avg_costs,
                model_rankings,
            })
        })
        .await?
    }

    async fn get_model_stats(&self, window_hours: u32) -> anyhow::Result<Vec<ModelPerformanceStats>> {
        let conn = self.conn.clone();
        tokio::task::spawn_blocking(move || {
            let conn = conn.lock().unwrap_or_else(|e| e.into_inner());
            let cutoff = if window_hours == 0 {
                0i64
            } else {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs() as i64;
                now - (window_hours as i64 * 3600)
            };

            let mut stmt = conn.prepare(
                "SELECT model,
                        COUNT(*) as total_requests,
                        SUM(CASE WHEN success = 1 THEN 1 ELSE 0 END) as success_count,
                        AVG(latency_ms) as avg_latency,
                        CAST(AVG(cost) AS INTEGER) as avg_cost
                 FROM execution_records
                 WHERE timestamp >= ?1
                 GROUP BY model",
            )?;

            let rows = stmt.query_map([cutoff], |row| {
                let model: String = row.get(0)?;
                let total_requests: i64 = row.get(1)?;
                let success_count: i64 = row.get(2)?;
                let avg_latency_ms: f64 = row.get::<_, Option<f64>>(3)?.unwrap_or(0.0);
                let avg_cost = crate::types::NanoUSD::from_nanos(row.get::<_, Option<i64>>(4)?.unwrap_or(0).max(0) as u64);

                Ok(ModelPerformanceStats {
                    model,
                    total_requests: total_requests as u64,
                    success_count: success_count as u64,
                    avg_latency_ms,
                    avg_cost,
                })
            })?;

            let mut stats = Vec::new();
            for r in rows {
                stats.push(r?);
            }
            Ok(stats)
        })
        .await?
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Intent;

    fn make_record(
        model: &str,
        provider: &str,
        intent: Intent,
        latency_ms: u64,
        tokens: u32,
        cost_usd: f64,
        success: bool,
    ) -> ExecutionRecord {
        ExecutionRecord {
            record_id: uuid::Uuid::new_v4(),
            plan_id: uuid::Uuid::new_v4(),
            node_id: uuid::Uuid::new_v4(),
            model: model.to_string(),
            provider: provider.to_string(),
            intent,
            latency_ms,
            tokens,
            cost: crate::types::NanoUSD::from_nanos((cost_usd * 1_000_000_000.0) as u64),
            success,
            timestamp: 1000000,
        }
    }

    #[tokio::test]
    async fn test_wal_journal_mode() {
        let tmp = std::env::temp_dir().join(format!("fusion_wal_test_{}.db", std::process::id()));
        let _ = std::fs::remove_file(&tmp);

        let repo = SqliteEvidenceRepository::new(tmp.to_str().unwrap()).unwrap();
        drop(repo);

        let check = Connection::open(&tmp).unwrap();
        let mode: String = check
            .query_row("PRAGMA journal_mode", [], |row| row.get(0))
            .unwrap();
        let _ = std::fs::remove_file(&tmp);
        assert_eq!(mode, "wal", "journal_mode should be WAL");
    }

    #[tokio::test]
    async fn test_snapshot_aggregation() {
        let tmp = std::env::temp_dir()
            .join(format!("fusion_es01_{}.db", std::process::id()));
        let _ = std::fs::remove_file(&tmp);

        let repo = SqliteEvidenceRepository::new(tmp.to_str().unwrap()).unwrap();

        repo.record(make_record("gpt-4", "openai", Intent::Code, 100, 50, 0.01, true))
            .await
            .unwrap();
        repo.record(make_record("gpt-4", "openai", Intent::Code, 200, 100, 0.02, false))
            .await
            .unwrap();
        repo.record(make_record("claude-3", "anthropic", Intent::Debug, 150, 75, 0.015, true))
            .await
            .unwrap();
        repo.record(make_record("claude-3", "anthropic", Intent::Debug, 50, 25, 0.005, true))
            .await
            .unwrap();

        let snap = repo.snapshot().await.unwrap();

        assert_eq!(snap.record_count, 4);

        assert_eq!(*snap.success_rates.get("gpt-4::Code").unwrap(), 0.5);
        assert_eq!(*snap.success_rates.get("claude-3::Debug").unwrap(), 1.0);

        assert_eq!(*snap.avg_latencies.get("gpt-4").unwrap(), 150.0);
        assert_eq!(*snap.avg_latencies.get("claude-3").unwrap(), 100.0);

        assert_eq!(snap.avg_costs.get("gpt-4").unwrap().to_usd_f64(), 0.015);
        assert_eq!(snap.avg_costs.get("claude-3").unwrap().to_usd_f64(), 0.01);

        assert_eq!(snap.model_rankings.len(), 2);
        assert_eq!(snap.model_rankings[0], "claude-3");
        assert_eq!(snap.model_rankings[1], "gpt-4");

        let _ = std::fs::remove_file(&tmp);
    }

    #[tokio::test]
    async fn test_snapshot_cold_start() {
        let tmp = std::env::temp_dir()
            .join(format!("fusion_es02_{}.db", std::process::id()));
        let _ = std::fs::remove_file(&tmp);

        let repo = SqliteEvidenceRepository::new(tmp.to_str().unwrap()).unwrap();
        let snap = repo.snapshot().await.unwrap();

        assert_eq!(snap.record_count, 0);
        assert!(snap.success_rates.is_empty());
        assert!(snap.avg_latencies.is_empty());
        assert!(snap.avg_costs.is_empty());
        assert!(snap.model_rankings.is_empty());

        let _ = std::fs::remove_file(&tmp);
    }

    #[tokio::test]
    async fn test_record_execution() {
        let tmp = std::env::temp_dir()
            .join(format!("fusion_tl01_{}.db", std::process::id()));
        let _ = std::fs::remove_file(&tmp);

        let repo = SqliteEvidenceRepository::new(tmp.to_str().unwrap()).unwrap();
        let rec = make_record("gpt-4", "openai", Intent::General, 100, 50, 0.01, true);
        repo.record(rec).await.unwrap();

        let snap = repo.snapshot().await.unwrap();
        assert_eq!(snap.record_count, 1);
        assert_eq!(snap.success_rates.len(), 1);
        assert_eq!(snap.model_rankings.len(), 1);

        let _ = std::fs::remove_file(&tmp);
    }

    #[tokio::test]
    async fn test_get_model_stats() {
        let tmp = std::env::temp_dir()
            .join(format!("fusion_tl02_{}.db", std::process::id()));
        let _ = std::fs::remove_file(&tmp);

        let repo = SqliteEvidenceRepository::new(tmp.to_str().unwrap()).unwrap();

        repo.record(make_record("gpt-4", "openai", Intent::Code, 100, 50, 0.01, true))
            .await
            .unwrap();
        repo.record(make_record("gpt-4", "openai", Intent::Debug, 200, 100, 0.02, false))
            .await
            .unwrap();
        repo.record(make_record("claude-3", "anthropic", Intent::Code, 150, 75, 0.015, true))
            .await
            .unwrap();

        let stats = repo.get_model_stats(0).await.unwrap();

        assert_eq!(stats.len(), 2);

        let gpt4 = stats.iter().find(|s| s.model == "gpt-4").unwrap();
        assert_eq!(gpt4.total_requests, 2);
        assert_eq!(gpt4.success_count, 1);

        let claude = stats.iter().find(|s| s.model == "claude-3").unwrap();
        assert_eq!(claude.total_requests, 1);
        assert_eq!(claude.success_count, 1);

        let _ = std::fs::remove_file(&tmp);
    }
}

