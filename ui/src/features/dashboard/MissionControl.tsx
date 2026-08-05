import { useQuery } from '@tanstack/react-query';
import { Cpu, Gauge, Layers, ShieldCheck, XCircle, Zap } from 'lucide-react';
import { api } from '../../lib/api';
import { EmptyState, ErrorBanner, GlassCard, PageHeader, Skeleton, StatCard, StatusPill } from '../../components/ui';

export function MissionControl() {
  const { data, isLoading, error, refetch } = useQuery({ queryKey: ['dashboard'], queryFn: api.getDashboard });

  return (
    <div className="flex flex-col gap-6">
      <PageHeader
        title="Mission Control"
        subtitle="Governance metrics and architecture KPIs from the Studio BFF."
        right={
          data ? <StatusPill status={data.overview.status} /> : undefined
        }
      />

      {error && <ErrorBanner message="Dashboard unavailable — is the FusionStudio server running?" onRetry={() => refetch()} />}

      {isLoading && !data ? (
        <div className="grid grid-cols-2 gap-4 lg:grid-cols-5">
          {Array.from({ length: 5 }).map((_, i) => (
            <Skeleton key={i} className="h-28" />
          ))}
        </div>
      ) : data ? (
        <>
          {/* Overview stats */}
          <div className="grid grid-cols-2 gap-4 lg:grid-cols-5">
            <StatCard label="Status" value={data.overview.status} tone="success" mono={false} />
            <StatCard label="Active Executions" value={data.overview.active_executions} tone="info" />
            <StatCard label="Total Requests" value={data.overview.total_requests.toLocaleString()} />
            <StatCard label="Avg Latency" value={`${data.overview.avg_latency_ms}ms`} sub="p50 across providers" />
            <StatCard label="Healthy Providers" value={`${data.overview.healthy_providers} / 3`} tone="success" />
          </div>

          {/* Architecture KPIs */}
          <GlassCard>
            <div className="mb-4 flex items-center gap-2">
              <Layers className="h-4 w-4 text-accent" />
              <h3 className="text-sm font-bold text-gray-100">Architecture KPIs</h3>
            </div>
            <div className="grid grid-cols-2 gap-4 md:grid-cols-5">
              <Kpi label="Planner SLO" icon={Zap} value={`${data.architecture_kpis.planner_slo_ms}ms`} />
              <Kpi label="Compiler SLO" icon={Cpu} value={`${data.architecture_kpis.compiler_slo_ms}ms`} />
              <Kpi label="Scheduler SLO" icon={Gauge} value={`${data.architecture_kpis.scheduler_slo_ms}ms`} />
              <Kpi
                label="Bypass Violations"
                icon={XCircle}
                value={String(data.architecture_kpis.zero_bypass_violations)}
                ok={data.architecture_kpis.zero_bypass_violations === 0}
              />
              <Kpi
                label="Conformance"
                icon={ShieldCheck}
                value={`${(data.architecture_kpis.conformance_pass_rate * 100).toFixed(0)}%`}
                ok={data.architecture_kpis.conformance_pass_rate >= 1}
              />
            </div>
          </GlassCard>

          {/* Provider health */}
          <GlassCard>
            <div className="mb-4 flex items-center gap-2">
              <Zap className="h-4 w-4 text-accent" />
              <h3 className="text-sm font-bold text-gray-100">Provider Fleet Health</h3>
            </div>
            <div className="flex flex-col divide-y divide-white/5">
              {data.providers_health.map((p) => (
                <div key={p.name} className="flex items-center justify-between gap-4 py-3">
                  <div className="flex items-center gap-3">
                    <span className="mono text-sm font-semibold text-gray-200">{p.name}</span>
                  </div>
                  <div className="flex items-center gap-3">
                    <span className="mono text-xs text-gray-500">{p.latency_ms}ms</span>
                    <StatusPill status={p.status} />
                  </div>
                </div>
              ))}
            </div>
          </GlassCard>
        </>
      ) : (
        <EmptyState title="No dashboard data" />
      )}
    </div>
  );
}

function Kpi({
  label,
  icon: Icon,
  value,
  ok,
}: {
  label: string;
  icon: typeof Cpu;
  value: string;
  ok?: boolean;
}) {
  return (
    <div className="rounded-xl border border-white/10 bg-white/[0.02] p-3.5">
      <div className="flex items-center gap-2 text-xs font-semibold uppercase tracking-wide text-gray-500">
        <Icon className="h-3.5 w-3.5 text-gray-400" />
        {label}
      </div>
      <p className={`mono mt-2 text-xl font-bold ${ok === false ? 'text-rose-400' : 'text-gray-100'}`}>{value}</p>
    </div>
  );
}