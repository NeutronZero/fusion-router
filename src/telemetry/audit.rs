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
            .map(|e| serde_json::to_string(e).unwrap_or_default())
            .collect::<Vec<_>>()
            .join("\n")
    }
}

impl Default for AuditLog {
    fn default() -> Self {
        Self::new(1000)
    }
}
