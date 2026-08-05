import { useQuery } from '@tanstack/react-query';
import { CheckCircle2, RefreshCcw, ShieldCheck, XCircle } from 'lucide-react';
import { api } from '../../lib/api';
import { EmptyState, ErrorBanner, GlassCard, PageHeader, Skeleton, StatusPill, toneOf } from '../../components/ui';

export function DiagnosticsView() {
  const { data, isLoading, error, refetch, isFetching } = useQuery({
    queryKey: ['diagnostics'],
    queryFn: api.getDiagnostics,
  });

  return (
    <div className="flex flex-col gap-6">
      <PageHeader
        title="Platform Diagnostics"
        subtitle="One-click system check of the FusionRouter gateway, database and provider connectivity."
        right={
          <button className="btn-secondary" onClick={() => refetch()} disabled={isFetching}>
            <RefreshCcw className={`h-4 w-4 ${isFetching ? 'animate-spin' : ''}`} />
            {isFetching ? 'Checking…' : 'Run Check'}
          </button>
        }
      />

      {error && <ErrorBanner message="Diagnostics unavailable — is the FusionStudio server running?" onRetry={() => refetch()} />}

      {isLoading && !data ? (
        <div className="glass-panel space-y-4 p-6">
          {Array.from({ length: 4 }).map((_, i) => (
            <Skeleton key={i} className="h-12" />
          ))}
        </div>
      ) : data ? (
        <GlassCard>
          <div className="mb-5 flex items-center justify-between">
            <div className="flex items-center gap-2">
              <ShieldCheck className="h-5 w-5 text-emerald-400" />
              <span className="font-semibold text-gray-100">System Check</span>
            </div>
            <StatusPill status={data.system_check === 'passed' ? 'Healthy' : data.system_check} />
          </div>
          <div className="flex flex-col divide-y divide-white/5">
            {data.checks.map((check) => (
              <div key={check.name} className="flex items-center justify-between gap-4 py-3.5">
                <div className="flex items-center gap-3">
                  {toneOf(check.status) === 'danger' ? (
                    <XCircle className="h-4 w-4 text-rose-400" />
                  ) : (
                    <CheckCircle2 className="h-4 w-4 text-emerald-400" />
                  )}
                  <span className="text-sm font-medium text-gray-200">{check.name}</span>
                </div>
                <div className="flex items-center gap-3">
                  <span className="mono text-xs text-gray-500">{check.latency_ms}ms</span>
                  <StatusPill status={check.status} />
                </div>
              </div>
            ))}
          </div>
        </GlassCard>
      ) : (
        <EmptyState title="No diagnostics data" />
      )}
    </div>
  );
}