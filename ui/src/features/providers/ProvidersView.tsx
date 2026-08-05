import { useState } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { CheckCircle2, Globe, Loader2, Plus, Server, Zap } from 'lucide-react';
import { api, PROVIDER_TYPES } from '../../lib/api';
import {
  Badge,
  EmptyState,
  ErrorBanner,
  GlassCard,
  PageHeader,
  Skeleton,
  StatusPill,
} from '../../components/ui';

export function ProvidersView() {
  const queryClient = useQueryClient();
  const { data, isLoading, error, refetch } = useQuery({ queryKey: ['providers'], queryFn: api.getProviders });
  const [adding, setAdding] = useState(false);

  const createMutation = useMutation({
    mutationFn: api.createProvider,
    onSuccess: () => {
      setAdding(false);
      queryClient.invalidateQueries({ queryKey: ['providers'] });
    },
  });

  return (
    <div className="flex flex-col gap-6">
      <PageHeader
        title="Provider Fleet"
        subtitle="Model providers with hot-reload status. Credentials are encrypted with AES-256-GCM before storage."
        right={
          <button className="btn-primary" onClick={() => setAdding((v) => !v)}>
            {adding ? <Loader2 className="h-4 w-4 animate-spin" /> : <Plus className="h-4 w-4" />}
            {adding ? 'Close' : 'Add Provider'}
          </button>
        }
      />

      {adding && (
        <AddProviderForm
          submitting={createMutation.isPending}
          error={createMutation.isError ? 'Could not create provider. Check server connection.' : null}
          onCancel={() => setAdding(false)}
          onSubmit={(values) => createMutation.mutate(values)}
        />
      )}

      {error && <ErrorBanner message="Could not load providers — is the FusionStudio server running?" onRetry={() => refetch()} />}

      {isLoading && !data ? (
        <div className="grid grid-cols-1 gap-4 md:grid-cols-2 xl:grid-cols-3">
          {Array.from({ length: 6 }).map((_, i) => (
            <Skeleton key={i} className="h-44" />
          ))}
        </div>
      ) : data && data.providers.length > 0 ? (
        <div className="grid grid-cols-1 gap-4 md:grid-cols-2 xl:grid-cols-3">
          {data.providers.map((p) => (
            <ProviderCard key={p.id} {...p} />
          ))}
        </div>
      ) : (
        <EmptyState title="No providers configured yet" description="Add your first provider to start routing compiled workflows." />
      )}
    </div>
  );
}

function ProviderCard(props: {
  id: string;
  name: string;
  provider_type: string;
  status: string;
  latency_ms: number;
  models_count: number;
  is_default: boolean;
  capabilities: string[];
  base_url: string | null;
}) {
  const { id, name, provider_type, status, latency_ms, models_count, is_default, capabilities, base_url } = props;
  const test = useMutation({ mutationFn: api.testProvider });
  const last = test.data && test.data.provider_id === id ? test.data : null;

  return (
    <GlassCard hover className="flex flex-col gap-3">
      <div className="flex items-start justify-between gap-2">
        <div className="flex items-center gap-3">
          <div className="flex h-10 w-10 items-center justify-center rounded-xl bg-white/5">
            <Server className="h-5 w-5 text-accent" />
          </div>
          <div>
            <p className="font-semibold text-gray-100">{name}</p>
            <p className="mono text-[11px] text-gray-500">{provider_type}</p>
          </div>
        </div>
        <div className="flex flex-col items-end gap-1">{is_default && <Badge>Default</Badge>}</div>
      </div>

      <div className="flex items-center justify-between text-xs text-gray-400">
        <span className="flex items-center gap-1.5">
          <Zap className="h-3.5 w-3.5 text-gray-500" /> {latency_ms}ms latency
        </span>
        <span>{models_count} models</span>
      </div>

      {base_url && (
        <p className="flex items-center gap-1.5 text-[11px] text-gray-500">
          <Globe className="h-3 w-3" /> {base_url}
        </p>
      )}

      <div className="flex flex-wrap gap-1.5">
        {capabilities.map((c) => (
          <Badge key={c}>{c}</Badge>
        ))}
      </div>

      <div className="mt-auto flex items-center justify-between gap-2 border-t border-white/5 pt-3">
        <StatusPill status={status} />
        <button
          className="btn-ghost gap-1.5 !py-1 text-xs"
          disabled={test.isPending}
          onClick={() => test.mutate({ provider_id: id })}
        >
          {test.isPending ? (
            <Loader2 className="h-3.5 w-3.5 animate-spin" />
          ) : last ? (
            <CheckCircle2 className="h-3.5 w-3.5 text-emerald-400" />
          ) : (
            <CheckCircle2 className="h-3.5 w-3.5" />
          )}
          {last ? `Healthy · ${last.latency_ms}ms` : 'Test Connection'}
        </button>
      </div>
    </GlassCard>
  );
}

function AddProviderForm({
  onSubmit,
  submitting,
  error,
  onCancel,
}: {
  onSubmit: (v: {
    name: string;
    provider_type: string;
    api_key: string | null;
    base_url: string | null;
    set_as_default: boolean;
  }) => void;
  submitting: boolean;
  error: string | null;
  onCancel: () => void;
}) {
  const [name, setName] = useState('');
  const [providerType, setProviderType] = useState<string>(PROVIDER_TYPES[0]);
  const [baseUrl, setBaseUrl] = useState('');
  const [apiKey, setApiKey] = useState('');
  const [setAsDefault, setSetAsDefault] = useState(true);

  return (
    <GlassCard className="!border-accent/30">
      <form
        className="grid grid-cols-1 gap-4 md:grid-cols-2"
        onSubmit={(e) => {
          e.preventDefault();
          onSubmit({
            name,
            provider_type: providerType,
            api_key: apiKey || null,
            base_url: baseUrl || null,
            set_as_default: setAsDefault,
          });
        }}
      >
        <div>
          <label className="label-dark">Name</label>
          <input className="input-dark" value={name} onChange={(e) => setName(e.target.value)} placeholder="Ollama Local Engine" required />
        </div>
        <div>
          <label className="label-dark">Type</label>
          <select className="input-dark" value={providerType} onChange={(e) => setProviderType(e.target.value)}>
            {PROVIDER_TYPES.map((t) => (
              <option key={t} value={t}>
                {t}
              </option>
            ))}
          </select>
        </div>
        <div>
          <label className="label-dark">Base URL</label>
          <input className="input-dark" value={baseUrl} onChange={(e) => setBaseUrl(e.target.value)} placeholder="http://localhost:11434" />
        </div>
        <div>
          <label className="label-dark">API Key (optional)</label>
          <input className="input-dark" type="password" value={apiKey} onChange={(e) => setApiKey(e.target.value)} placeholder="Encrypted with AES-256-GCM" />
        </div>
        <div className="flex items-center gap-2">
          <label className="flex items-center gap-2 text-sm text-gray-300">
            <input type="checkbox" checked={setAsDefault} onChange={(e) => setSetAsDefault(e.target.checked)} className="accent-accent" />
            Set as default provider
          </label>
        </div>
        {error && <p className="text-sm text-rose-400">{error}</p>}
        <div className="flex items-center justify-end gap-2 md:col-span-2">
          <button type="button" className="btn-secondary" onClick={onCancel}>
            Cancel
          </button>
          <button type="submit" className="btn-primary" disabled={submitting}>
            {submitting && <Loader2 className="h-4 w-4 animate-spin" />}
            Add Provider
          </button>
        </div>
      </form>
    </GlassCard>
  );
}