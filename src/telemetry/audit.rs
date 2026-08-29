use parking_lot::Mutex;
use serde_json::Value;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AuditEntry {
    pub timestamp: i64,
    pub request_id: String,
    pub user_id: Option<String>,
    pub action: String,
    pub result: String,
    pub details: Value,
}

pub struct AuditLog {
    entries: Mutex<Vec<AuditEntry>>,
    max_entries: usize,
}

impl AuditLog {
    pub fn new(max_entries: usize) -> Self {
        Self {
            entries: Mutex::new(Vec::new()),
            max_entries,
        }
    }

    pub fn record(&self, entry: AuditEntry) {
        let mut entries = self.entries.lock();
        if entries.len() >= self.max_entries {
            entries.remove(0);
        }
        entries.push(entry);
    }

    pub fn entries(&self) -> Vec<AuditEntry> {
        self.entries.lock().clone()
    }

    pub fn to_jsonl(&self) -> String {
        let entries = self.entries.lock();
        entries
            .iter()
            .filter_map(|e| match serde_json::to_string(e) {
                Ok(s) => Some(s),
                Err(err) => {
                    tracing::error!(error = %err, "audit entry serialization failed; entry dropped");
                    None
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Like `to_jsonl` but fails closed: returns Err if any entry cannot be
    /// serialized, so callers that require a complete audit trail can reject
    /// the write instead of emitting a partial file.
    pub fn try_to_jsonl(&self) -> Result<String, serde_json::Error> {
        let entries = self.entries.lock();
        let mut out = Vec::with_capacity(entries.len());
        for e in entries.iter() {
            out.push(serde_json::to_string(e)?);
        }
        Ok(out.join("\n"))
    }
}

impl Default for AuditLog {
    fn default() -> Self {
        Self::new(1000)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_audit_log_record_and_retrieve() {
        let log = AuditLog::new(10);
        let entry = AuditEntry {
            timestamp: 1,
            request_id: "req-1".into(),
            user_id: Some("user-1".into()),
            action: "test".into(),
            result: "ok".into(),
            details: serde_json::json!({"key": "value"}),
        };
        log.record(entry.clone());
        let entries = log.entries();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].request_id, "req-1");
        assert_eq!(entries[0].user_id.as_deref(), Some("user-1"));
    }

    #[test]
    fn test_audit_log_max_entries() {
        let log = AuditLog::new(3);
        for i in 0..5 {
            log.record(AuditEntry {
                timestamp: i,
                request_id: format!("req-{i}"),
                user_id: None,
                action: "test".into(),
                result: "ok".into(),
                details: serde_json::json!({}),
            });
        }
        let entries = log.entries();
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].request_id, "req-2");
        assert_eq!(entries[2].request_id, "req-4");
    }

    #[test]
    fn test_audit_log_to_jsonl() {
        let log = AuditLog::new(10);
        log.record(AuditEntry {
            timestamp: 1,
            request_id: "a".into(),
            user_id: None,
            action: "create".into(),
            result: "ok".into(),
            details: serde_json::json!({}),
        });
        log.record(AuditEntry {
            timestamp: 2,
            request_id: "b".into(),
            user_id: Some("u".into()),
            action: "delete".into(),
            result: "ok".into(),
            details: serde_json::json!({"x": 1}),
        });
        let jsonl = log.to_jsonl();
        let lines: Vec<&str> = jsonl.lines().collect();
        assert_eq!(lines.len(), 2);
        for line in &lines {
            let parsed: serde_json::Value = serde_json::from_str(line).unwrap();
            assert!(parsed.is_object());
        }
    }

    #[test]
    fn test_audit_log_default_max() {
        let log = AuditLog::default();
        assert_eq!(log.max_entries, 1000);
    }
}
