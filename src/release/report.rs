use crate::release::gate::duration_serde;
use crate::release::gate::GateResult;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GateReport {
    pub version: String,
    pub overall: bool,
    #[serde(with = "duration_serde")]
    pub duration: Duration,
    pub timestamp: DateTime<Utc>,
    pub gates: Vec<GateResult>,
}

impl GateReport {
    pub fn new(results: Vec<GateResult>, version: String) -> Self {
        let overall = results.iter().all(|r| r.passed);
        let duration = results
            .iter()
            .fold(Duration::ZERO, |acc, r| acc + r.duration);
        let timestamp = Utc::now();

        Self {
            version,
            overall,
            duration,
            timestamp,
            gates: results,
        }
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_else(|_| "{}".to_string())
    }

    pub fn to_text(&self) -> String {
        let mut output = String::new();
        output.push_str(&format!("Release Gate Report v{}\n", self.version));
        output.push_str(&format!("Timestamp: {}\n", self.timestamp));
        output.push_str(&format!(
            "Overall: {}\n\n",
            if self.overall { "PASS" } else { "FAIL" }
        ));

        for gate in &self.gates {
            let status = if gate.passed { "PASS" } else { "FAIL" };
            output.push_str(&format!(
                "[{}] {} - {}\n",
                status, gate.gate_id, gate.summary
            ));
        }

        output.push_str(&format!("\nDuration: {:.2?}", self.duration));

        output
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::release::gate::GateCheck;
    use crate::release::gate::GateId;

    fn make_result(gate_id: GateId, passed: bool) -> GateResult {
        GateResult {
            gate_id,
            passed,
            summary: format!("{} check", gate_id),
            details: vec![GateCheck {
                name: "check".into(),
                passed,
                message: "done".into(),
            }],
            duration: Duration::from_secs(1),
        }
    }

    #[test]
    fn test_report_overall_all_pass() {
        let results = vec![make_result(GateId::Sdk1, true)];
        let report = GateReport::new(results, "1.0.0".into());
        assert!(report.overall);
    }

    #[test]
    fn test_report_overall_any_fail() {
        let results = vec![
            make_result(GateId::Sdk1, true),
            make_result(GateId::Replay1, false),
        ];
        let report = GateReport::new(results, "1.0.0".into());
        assert!(!report.overall);
    }

    #[test]
    fn test_report_to_json_contains_overall() {
        let results = vec![make_result(GateId::Sdk1, true)];
        let report = GateReport::new(results, "1.0.0".into());
        let json = report.to_json();
        assert!(json.contains("overall"));
    }

    #[test]
    fn test_report_to_text_shows_pass_fail() {
        let results = vec![
            make_result(GateId::Sdk1, true),
            make_result(GateId::Replay1, false),
        ];
        let report = GateReport::new(results, "1.0.0".into());
        let text = report.to_text();
        assert!(text.contains("PASS"));
        assert!(text.contains("FAIL"));
    }
}
