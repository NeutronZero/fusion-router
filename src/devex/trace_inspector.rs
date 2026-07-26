use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct DiagnosticInfo {
    pub level: String,
    pub message: String,
}

pub struct TraceInspector {
    traces: Vec<DiagnosticInfo>,
}

impl Default for TraceInspector {
    fn default() -> Self {
        Self::new()
    }
}

impl TraceInspector {
    pub fn new() -> Self {
        Self { traces: Vec::new() }
    }

    pub fn record(&mut self, level: &str, message: &str) {
        self.traces.push(DiagnosticInfo {
            level: level.to_string(),
            message: message.to_string(),
        });
    }

    pub fn view_diagnostics(&self) -> String {
        let mut output = String::from("Diagnostics Viewer:\n");
        for (i, trace) in self.traces.iter().enumerate() {
            output.push_str(&format!("[{}] {}: {}\n", i, trace.level, trace.message));
        }
        output
    }
}
