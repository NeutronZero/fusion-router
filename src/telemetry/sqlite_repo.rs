use async_trait::async_trait;
use rusqlite::{params, Connection};
use std::collections::HashMap;
use std::sync::Mutex;
use tracing::info;

use crate::telemetry::EvidenceRepository;
use crate::types::{EvidenceSnapshot, ExecutionRecord};

pub struct SqliteEvidenceRepository {
    conn: Mutex<Connection>,
}

impl SqliteEvidenceRepository {
    pub fn new(path: &str) -> anyhow::Result<Self> {
        let conn = Connection::open(path)?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS execution_records (
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
            conn: Mutex::new(conn),
        })
    }
}

#[async_trait]
impl EvidenceRepository for SqliteEvidenceRepository {
    async fn record(&self, entry: ExecutionRecord) -> anyhow::Result<()> {
        let conn = self.conn.lock().unwrap();
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
    }

    async fn snapshot(&self) -> anyhow::Result<EvidenceSnapshot> {
        let conn = self.conn.lock().unwrap();

        let record_count: u64 = conn
            .query_row("SELECT COUNT(*) FROM execution_records", [], |row| {
                row.get::<_, i64>(0)
            })? as u64;

        let mut success_rates: HashMap<String, f64> = HashMap::new();
        {
            let mut stmt = conn.prepare(
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
            let mut stmt = conn.prepare(
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
            let mut stmt = conn.prepare(
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
            let mut stmt = conn.prepare(
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

        Ok(EvidenceSnapshot {
            record_count,
            success_rates,
            avg_latencies,
            avg_costs,
            model_rankings,
        })
    }
}
