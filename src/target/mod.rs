use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExecutionEnvironment {
    Local,
    Cloud,
    Kubernetes,
    GpuCluster,
    Edge,
    AirGapped,
    Browser,
    Hybrid,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SchedulerKind {
    DepthFirst,
    BreadthFirst,
    CriticalPath,
    LatencyOptimized,
    CostOptimized,
    Distributed,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct ResourceLimits {
    pub max_memory_bytes: Option<u64>,
    pub max_cpu_cores: Option<f32>,
    pub max_parallelism: Option<u32>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct NetworkConstraints {
    pub allow_egress: bool,
    pub allowed_domains: Vec<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct SecurityProfile {
    pub sandbox_required: bool,
    pub attestation_required: bool,
}

/// Provider-independent runtime placement constraints (v0.13 contract 4).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionTarget {
    pub environment: ExecutionEnvironment,
    pub resource_limits: ResourceLimits,
    pub network_constraints: NetworkConstraints,
    pub security_profile: SecurityProfile,
    pub preferred_scheduler: SchedulerKind,
}

impl Default for ExecutionTarget {
    fn default() -> Self {
        Self {
            environment: ExecutionEnvironment::Local,
            resource_limits: ResourceLimits::default(),
            network_constraints: NetworkConstraints::default(),
            security_profile: SecurityProfile::default(),
            preferred_scheduler: SchedulerKind::DepthFirst,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_target_is_local_depth_first() {
        let t = ExecutionTarget::default();
        assert_eq!(t.environment, ExecutionEnvironment::Local);
        assert_eq!(t.preferred_scheduler, SchedulerKind::DepthFirst);
    }

    #[test]
    fn serde_round_trip_preserves_target() {
        let t = ExecutionTarget {
            environment: ExecutionEnvironment::Kubernetes,
            resource_limits: ResourceLimits {
                max_memory_bytes: Some(1 << 30),
                max_parallelism: Some(8),
                ..ResourceLimits::default()
            },
            network_constraints: NetworkConstraints {
                allow_egress: true,
                allowed_domains: vec!["api.example.com".into()],
            },
            security_profile: SecurityProfile {
                sandbox_required: true,
                attestation_required: true,
            },
            preferred_scheduler: SchedulerKind::LatencyOptimized,
        };
        let json = serde_json::to_string(&t).unwrap();
        let back: ExecutionTarget = serde_json::from_str(&json).unwrap();
        assert_eq!(back.environment, ExecutionEnvironment::Kubernetes);
        assert_eq!(back.preferred_scheduler, SchedulerKind::LatencyOptimized);
        assert_eq!(back.network_constraints.allowed_domains.len(), 1);
        assert!(back.security_profile.attestation_required);
    }

    #[test]
    fn all_environments_round_trip() {
        for env in [
            ExecutionEnvironment::Local,
            ExecutionEnvironment::Cloud,
            ExecutionEnvironment::Kubernetes,
            ExecutionEnvironment::GpuCluster,
            ExecutionEnvironment::Edge,
            ExecutionEnvironment::AirGapped,
            ExecutionEnvironment::Browser,
            ExecutionEnvironment::Hybrid,
        ] {
            let json = serde_json::to_string(&env).unwrap();
            let back: ExecutionEnvironment = serde_json::from_str(&json).unwrap();
            assert_eq!(back, env);
        }
    }

    #[test]
    fn all_schedulers_round_trip() {
        for sched in [
            SchedulerKind::DepthFirst,
            SchedulerKind::BreadthFirst,
            SchedulerKind::CriticalPath,
            SchedulerKind::LatencyOptimized,
            SchedulerKind::CostOptimized,
            SchedulerKind::Distributed,
        ] {
            let json = serde_json::to_string(&sched).unwrap();
            let back: SchedulerKind = serde_json::from_str(&json).unwrap();
            assert_eq!(back, sched);
        }
    }

    #[test]
    fn target_rejects_provider_fields() {
        let t = ExecutionTarget::default();
        let mut json = serde_json::to_string(&t).unwrap();
        let trimmed = json.trim_end_matches('}');
        json = format!("{trimmed}, \"provider\":\"openai\"}}");
        assert!(serde_json::from_str::<ExecutionTarget>(&json).is_err());
    }
}
