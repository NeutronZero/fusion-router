use serde::{Deserialize, Serialize};
use std::path::Path;
use chrono::{DateTime, Utc};
use crate::release::gate::{GateError, GateId};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Waiver {
    pub id: String,
    pub gate: GateId,
    #[serde(default)]
    pub artifact: Option<String>,
    pub reason: String,
    pub expires: DateTime<Utc>,
    pub approved_by: String,
}

impl Waiver {
    pub fn is_active(&self, now: DateTime<Utc>) -> bool {
        self.expires > now
    }

    pub fn matches(&self, gate_id: GateId, artifact_name: Option<&str>) -> bool {
        if self.gate != gate_id {
            return false;
        }
        match (&self.artifact, artifact_name) {
            (Some(waived_art), Some(target_art)) => waived_art == target_art || waived_art == "*",
            (Some(_), None) => false,
            (None, _) => true,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct WaiverSet {
    #[serde(default)]
    pub waivers: Vec<Waiver>,
}

impl WaiverSet {
    pub fn find_active_waiver(
        &self,
        gate_id: GateId,
        artifact_name: Option<&str>,
        now: DateTime<Utc>,
    ) -> Option<&Waiver> {
        self.waivers.iter().find(|w| w.is_active(now) && w.matches(gate_id, artifact_name))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WaiverEvaluation {
    pub waiver: Waiver,
    pub active: bool,
    pub gate: GateId,
}

pub fn load_waivers_from_yaml(path: &Path) -> Result<WaiverSet, GateError> {
    if !path.exists() {
        return Ok(WaiverSet::default());
    }
    let content = std::fs::read_to_string(path)
        .map_err(|e| GateError::ExecutionFailed(format!("read waivers file {}: {e}", path.display())))?;
    serde_yaml::from_str(&content)
        .map_err(|e| GateError::ExecutionFailed(format!("parse waivers file {}: {e}", path.display())))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn test_waiver_active_check() {
        let future = Utc.with_ymd_and_hms(2030, 1, 1, 0, 0, 0).unwrap();
        let past = Utc.with_ymd_and_hms(2020, 1, 1, 0, 0, 0).unwrap();
        let now = Utc.with_ymd_and_hms(2026, 7, 27, 0, 0, 0).unwrap();

        let active_waiver = Waiver {
            id: "waiver-1".into(),
            gate: GateId::Provider1,
            artifact: Some("openai".into()),
            reason: "testing".into(),
            expires: future,
            approved_by: "architecture".into(),
        };

        let expired_waiver = Waiver {
            id: "waiver-2".into(),
            gate: GateId::Provider1,
            artifact: Some("openai".into()),
            reason: "testing".into(),
            expires: past,
            approved_by: "architecture".into(),
        };

        assert!(active_waiver.is_active(now));
        assert!(!expired_waiver.is_active(now));
    }

    #[test]
    fn test_waiver_matching() {
        let future = Utc.with_ymd_and_hms(2030, 1, 1, 0, 0, 0).unwrap();
        let waiver = Waiver {
            id: "waiver-1".into(),
            gate: GateId::Provider1,
            artifact: Some("openai".into()),
            reason: "testing".into(),
            expires: future,
            approved_by: "architecture".into(),
        };

        assert!(waiver.matches(GateId::Provider1, Some("openai")));
        assert!(!waiver.matches(GateId::Provider1, Some("anthropic")));
        assert!(!waiver.matches(GateId::Plugin1, Some("openai")));
    }
}
