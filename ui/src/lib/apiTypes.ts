// Typed mirrors of the FusionStudio BFF API (crates/fusion-studio-api).

export interface HealthResponse {
  status: string;
  version: string;
  edition: string;
  laws_active: number;
}

export interface DashboardResponse {
  overview: {
    status: string;
    active_executions: number;
    total_requests: number;
    avg_latency_ms: number;
    healthy_providers: number;
  };
  architecture_kpis: {
    planner_slo_ms: number;
    compiler_slo_ms: number;
    scheduler_slo_ms: number;
    zero_bypass_violations: number;
    conformance_pass_rate: number;
  };
  providers_health: { name: string; status: string; latency_ms: number }[];
}

export interface StudioProviderInfo {
  id: string;
  name: string;
  provider_type: string;
  status: string;
  latency_ms: number;
  models_count: number;
  is_default: boolean;
  capabilities: string[];
  base_url: string | null;
}

export interface StudioProvidersResponse {
  providers: StudioProviderInfo[];
  default_provider_id: string;
}

export interface CreateStudioProviderRequest {
  name: string;
  provider_type: string;
  api_key?: string | null;
  base_url?: string | null;
  set_as_default?: boolean;
}

export interface CreateStudioProviderResponse {
  status: string;
  provider: {
    id: string;
    name: string;
    provider_type: string;
    base_url: string | null;
    is_default: boolean;
    key_encrypted: boolean;
  };
}

export interface TestStudioProviderRequest {
  provider_id: string;
  base_url?: string | null;
  api_key?: string | null;
}

export interface TestStudioProviderResponse {
  provider_id: string;
  status: string;
  latency_ms: number;
  models_discovered: string[];
  capabilities: string[];
  tested_at: string;
}

export interface StudioChatRequest {
  prompt: string;
  session_id?: string | null;
  provider_preference?: string | null;
}

export interface ExecutionBadge {
  execution_id: string;
  passes_executed: number;
  graph_id: string;
  status: string;
  inspector_url: string;
  replay_url: string;
}

export interface TimelineStage {
  stage: string;
  status: string;
  duration_ms: number;
}

export interface StudioChatResponse {
  execution_id: string;
  session_id: string;
  prompt: string;
  reply: string;
  provider: string;
  execution_badge: ExecutionBadge;
  timeline: TimelineStage[];
  compiler_report: Record<string, unknown>;
}

export interface ExecutionRecord {
  execution_id: string;
  intent: string;
  provider: string;
  model: string;
  status: string;
  duration_ms: number;
  cost: number;
  timestamp: string;
}

export interface StudioExecutionsResponse {
  executions: ExecutionRecord[];
}

export interface ReplayStep {
  pass_index: number;
  name: string;
  delta_nodes: number;
}

export interface StudioReplayResponse {
  replay_id: string;
  execution_id: string;
  bundle_file: string;
  total_passes: number;
  replay_status: string;
  placement_id: string;
  placement_policy: string;
  cluster_replay: {
    total_workers: number;
    worker_assignments: Record<string, string>;
    offline_simulation_side_effects: number;
  };
  steps: ReplayStep[];
}

export interface DiscoveredEngine {
  name: string;
  type: string;
  endpoint: string;
  models: string[];
  status: string;
}

export interface WizardDiscoverResponse {
  discovery_status: string;
  local_engines_found: number;
  discovered: DiscoveredEngine[];
}

export interface WizardCompleteResponse {
  wizard_completed: boolean;
  default_provider: string;
  status: string;
  completed_at: string;
}

export interface DiagnosticCheck {
  name: string;
  status: string;
  latency_ms: number;
}

export interface DiagnosticsResponse {
  system_check: string;
  checks: DiagnosticCheck[];
}

export interface ExplainRouteResponse {
  [key: string]: unknown;
}

export interface ApiErrorPayload {
  error?: string;
  message?: string;
}