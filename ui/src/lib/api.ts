import type {
  ApiErrorPayload,
  CreateStudioProviderRequest,
  CreateStudioProviderResponse,
  DashboardResponse,
  DiagnosticCheck,
  DiagnosticsResponse,
  DiscoveredEngine,
  ExplainRouteResponse,
  HealthResponse,
  StudioChatRequest,
  StudioChatResponse,
  StudioExecutionsResponse,
  StudioProviderInfo,
  StudioProvidersResponse,
  StudioReplayResponse,
  TestStudioProviderRequest,
  TestStudioProviderResponse,
  WizardCompleteResponse,
} from './apiTypes';

export class ApiError extends Error {
  readonly status: number;
  readonly payload: ApiErrorPayload | null;

  constructor(status: number, message: string, payload: ApiErrorPayload | null = null) {
    super(message);
    this.name = 'ApiError';
    this.status = status;
    this.payload = payload;
  }
}

async function request<T>(path: string, init?: RequestInit): Promise<T> {
  const res = await fetch(path, {
    ...init,
    headers: {
      'Content-Type': 'application/json',
      ...init?.headers,
    },
  });

  if (!res.ok) {
    let payload: ApiErrorPayload | null = null;
    try {
      payload = (await res.json()) as ApiErrorPayload;
    } catch {
      // non-JSON error body
    }
    throw new ApiError(res.status, payload?.error ?? payload?.message ?? `Request failed (${res.status})`, payload);
  }

  return (await res.json()) as T;
}

export const api = {
  getHealth: () => request<HealthResponse>('/api/v1/health'),
  getDashboard: () => request<DashboardResponse>('/api/v1/studio/dashboard'),
  getProviders: () => request<StudioProvidersResponse>('/api/v1/studio/providers'),
  createProvider: (body: CreateStudioProviderRequest) =>
    request<CreateStudioProviderResponse>('/api/v1/studio/providers', { method: 'POST', body: JSON.stringify(body) }),
  testProvider: (body: TestStudioProviderRequest) =>
    request<TestStudioProviderResponse>('/api/v1/studio/providers/test', { method: 'POST', body: JSON.stringify(body) }),
  chat: (body: StudioChatRequest) => request<StudioChatResponse>('/api/v1/studio/chat', { method: 'POST', body: JSON.stringify(body) }),
  getExecutions: () => request<StudioExecutionsResponse>('/api/v1/studio/executions'),
  getReplay: (executionId: string) => request<StudioReplayResponse>(`/api/v1/studio/replay/${encodeURIComponent(executionId)}`),
  getInspector: (executionId: string) =>
    request<Record<string, unknown>>(`/api/v1/studio/inspector/${encodeURIComponent(executionId)}`),
  discoverWizard: () =>
    request<{ discovery_status: string; local_engines_found: number; discovered: DiscoveredEngine[] }>(
      '/api/v1/studio/wizard/discover',
      { method: 'POST' },
    ),
  completeWizard: (default_provider: string) =>
    request<WizardCompleteResponse>('/api/v1/studio/wizard/complete', {
      method: 'POST',
      body: JSON.stringify({ default_provider }),
    }),
  explainRoute: (provider: string) =>
    request<ExplainRouteResponse>(`/api/v1/compiler/explain?provider=${encodeURIComponent(provider)}`),
  getDiagnostics: () => request<DiagnosticsResponse>('/api/v1/diagnostics'),
};

export type { DiagnosticCheck, StudioProviderInfo };

export const PROVIDER_TYPES = ['openrouter', 'anthropic', 'openai', 'ollama', 'lmstudio'] as const;