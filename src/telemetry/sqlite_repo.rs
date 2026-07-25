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
                 cost REAL NOT NULL,
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
                    entry.cost,
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
            {
                let mut stmt = tx.prepare(
                    "SELECT model, AVG(latency_ms) FROM execution_records GROUP BY model",
                )?;
                let rows = stmt.query_map([], |row| {
                    let model: String = row.get(0)?;
                    let avg: f64 = row.get(1)?;
                    Ok((model, avg))
                })?;
                for row in rows {
                    let (model, avg) = row?;
                    avg_latencies.insert(model, avg);
                }
            }

            let mut avg_costs: HashMap<String, f64> = HashMap::new();
            {
                let mut stmt = tx.prepare(
                    "SELECT model, AVG(cost) FROM execution_records GROUP BY model",
                )?;
                let rows = stmt.query_map([], |row| {
                    let model: String = row.get(0)?;
                    let avg: f64 = row.get(1)?;
                    Ok((model, avg))
                })?;
                for row in rows {
                    let (model, avg) = row?;
                    avg_costs.insert(model, avg);
                }
            }

            let mut model_rankings: Vec<String> = Vec::new();
            {
                let mut stmt = tx.prepare(
                    "SELECT model,
                            CAST(SUM(success) AS REAL) / CAST(COUNT(*) AS REAL) AS rate
                     FROM execution_records
                     GROUP BY model
                     ORDER BY rate DESC",
                )?;
                let rows = stmt.query_map([], |row| {
                    let model: String = row.get(0)?;
                    Ok(model)
                })?;
                for row in rows {
                    model_rankings.push(row?);
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
                        AVG(cost) as avg_cost
                 FROM execution_records
                 WHERE timestamp >= ?1
                 GROUP BY model",
            )?;

            let rows = stmt.query_map([cutoff], |row| {
                let model: String = row.get(0)?;
                let total_requests: i64 = row.get(1)?;
                let success_count: i64 = row.get(2)?;
                let avg_latency_ms: f64 = row.get::<_, Option<f64>>(3)?.unwrap_or(0.0);
                let avg_cost: f64 = row.get::<_, Option<f64>>(4)?.unwrap_or(0.0);

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
}

