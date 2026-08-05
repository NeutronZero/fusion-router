import { useMemo, useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import { GitBranch, Search } from 'lucide-react';
import { api } from '../../lib/api';
import { useAppStore } from '../../store/useAppStore';
import { EmptyState, ErrorBanner, GlassCard, PageHeader, Skeleton, StatusPill } from '../../components/ui';

export function HistoryView() {
  const { data, isLoading, error, refetch } = useQuery({ queryKey: ['executions'], queryFn: api.getExecutions });
  const [query, setQuery] = useState('');
  const setReplayExecutionId = useAppStore((s) => s.setReplayExecutionId);
  const setActiveView = useAppStore((s) => s.setActiveView);

  const rows = useMemo(() => {
    if (!data) return [];
    const q = query.trim().toLowerCase();
    if (!q) return data.executions;
    return data.executions.filter(
      (e) =>
        e.execution_id.toLowerCase().includes(q) ||
        e.intent.toLowerCase().includes(q) ||
        e.provider.toLowerCase().includes(q) ||
        e.model.toLowerCase().includes(q),
    );
  }, [data, query]);

  return (
    <div className="flex flex-col gap-6">
      <PageHeader
        title="Execution History"
        subtitle="Recent intent executions routed through the compiler pipeline."
        right={
          <div className="relative">
            <Search className="absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-gray-500" />
            <input
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              placeholder="Search executions…"
              className="input-dark !w-64 !py-2 pl-9"
            />
          </div>
        }
      />

      {error && <ErrorBanner message="Could not load execution history" onRetry={() => refetch()} />}

      {isLoading && !data ? (
        <div className="space-y-2">
          {Array.from({ length: 5 }).map((_, i) => (
            <Skeleton key={i} className="h-14" />
          ))}
        </div>
      ) : rows.length > 0 ? (
        <GlassCard className="!p-0">
          <table className="w-full text-left text-sm">
            <thead>
              <tr className="border-b border-white/10 text-[11px] uppercase tracking-wider text-gray-500">
                <th className="px-5 py-3 font-semibold">Execution</th>
                <th className="px-5 py-3 font-semibold">Intent</th>
                <th className="px-5 py-3 font-semibold">Provider</th>
                <th className="px-5 py-3 font-semibold">Model</th>
                <th className="px-5 py-3 font-semibold">Duration</th>
                <th className="px-5 py-3 font-semibold">Cost</th>
                <th className="px-5 py-3 font-semibold">Status</th>
                <th className="px-5 py-3 font-semibold">Replay</th>
              </tr>
            </thead>
            <tbody>
              {rows.map((e) => (
                <tr key={e.execution_id} className="border-b border-white/5 transition-colors last:border-0 hover:bg-white/[0.03]">
                  <td className="px-5 py-3">
                    <span className="mono text-xs text-accent">{e.execution_id}</span>
                  </td>
                  <td className="max-w-[260px] truncate px-5 py-3 text-gray-200">{e.intent}</td>
                  <td className="px-5 py-3 text-gray-400">{e.provider}</td>
                  <td className="px-5 py-3">
                    <span className="mono text-xs text-gray-300">{e.model}</span>
                  </td>
                  <td className="mono px-5 py-3 text-gray-300">{e.duration_ms}ms</td>
                  <td className="px-5 py-3 text-gray-300">${e.cost.toFixed(4)}</td>
                  <td className="px-5 py-3">
                    <StatusPill status={e.status} />
                  </td>
                  <td className="px-5 py-3">
                    <button
                      className="btn-ghost gap-1.5 !py-1 text-xs"
                      title={`Replay ${e.execution_id}`}
                      onClick={() => {
                        setReplayExecutionId(e.execution_id);
                        setActiveView('replay');
                      }}
                    >
                      <GitBranch className="h-3.5 w-3.5" /> Replay
                    </button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </GlassCard>
      ) : (
        <EmptyState title="No executions found" description={query ? 'Try a different search term.' : 'Compile a prompt in the Verification Chat to see executions here.'} />
      )}
    </div>
  );
}