import { useEffect } from 'react';
import { useQuery } from '@tanstack/react-query';
import { Boxes, FileArchive, GitBranch, HardDrive, Play, Server } from 'lucide-react';
import { api } from '../../lib/api';
import { useAppStore } from '../../store/useAppStore';
import { CodeBlock, EmptyState, ErrorBanner, GlassCard, PageHeader, Skeleton, StatusPill } from '../../components/ui';

export function ReplayEngineView() {
  const replayExecutionId = useAppStore((s) => s.replayExecutionId);
  const setReplayExecutionId = useAppStore((s) => s.setReplayExecutionId);
  const executions = useQuery({ queryKey: ['executions'], queryFn: api.getExecutions });

  useEffect(() => {
    if (!replayExecutionId && executions.data?.executions.length) {
      setReplayExecutionId(executions.data.executions[0].execution_id);
    }
  }, [executions.data, replayExecutionId, setReplayExecutionId]);

  const id = replayExecutionId ?? executions.data?.executions[0]?.execution_id ?? null;
  const { data, isLoading, error, refetch } = useQuery({
    queryKey: ['replay', id],
    queryFn: () => api.getReplay(id as string),
    enabled: !!id,
  });

  return (
    <div className="flex flex-col gap-6">
      <PageHeader
        title="Replay Engine"
        subtitle="Deterministic replay of executions from .fusion bundles — Timeline, Compiler Pass and Runtime modes."
        right={
          <select
            className="input-dark !w-56"
            value={id ?? ''}
            onChange={(e) => setReplayExecutionId(e.target.value)}
          >
            {(executions.data?.executions ?? []).map((e) => (
              <option key={e.execution_id} value={e.execution_id}>
                {e.execution_id} — {e.intent}
              </option>
            ))}
          </select>
        }
      />

      {error && <ErrorBanner message="Could not load replay bundle for this execution." onRetry={() => refetch()} />}

      {isLoading && !data ? (
        <div className="grid grid-cols-1 gap-4 lg:grid-cols-3">
          <Skeleton className="h-40 lg:col-span-2" />
          <Skeleton className="h-40" />
        </div>
      ) : data ? (
        <>
          {/* Job overview */}
          <div className="grid grid-cols-2 gap-4 lg:grid-cols-5">
            <InfoCard icon={Play} label="Replay Status" value={data.replay_status} pill />
            <InfoCard icon={GitBranch} label="Execution" value={data.execution_id} mono />
            <InfoCard icon={FileArchive} label="Bundle" value={data.bundle_file} mono />
            <InfoCard icon={Boxes} label="Compiler Passes" value={String(data.total_passes)} />
            <InfoCard icon={HardDrive} label="Placement" value={data.placement_id} mono />
          </div>

          <div className="grid grid-cols-1 gap-4 lg:grid-cols-3">
            {/* Pass steps */}
            <GlassCard className="lg:col-span-2">
              <h3 className="mb-4 text-sm font-bold text-gray-100">Replay Steps</h3>
              <div className="flex flex-col gap-2">
                {data.steps.map((step) => (
                  <div
                    key={step.pass_index}
                    className="flex items-center justify-between rounded-xl border border-white/10 bg-white/[0.02] px-4 py-2.5"
                  >
                    <span className="flex items-center gap-3">
                      <span className="mono text-xs text-gray-500">P{String(step.pass_index).padStart(2, '0')}</span>
                      <span className="text-sm text-gray-200">{step.name}</span>
                    </span>
                    <span className="mono text-xs text-gray-400">Δ {step.delta_nodes} nodes</span>
                  </div>
                ))}
              </div>
            </GlassCard>

            {/* Cluster replay */}
            <GlassCard>
              <div className="mb-4 flex items-center gap-2">
                <Server className="h-4 w-4 text-accent" />
                <h3 className="text-sm font-bold text-gray-100">Cluster Replay</h3>
              </div>
              <div className="space-y-3">
                <Row label="Workers" value={String(data.cluster_replay.total_workers)} />
                <Row label="Side Effects" value={String(data.cluster_replay.offline_simulation_side_effects)} />
                <Row label="Placement Policy" value={data.placement_policy} />
              </div>
              <div className="mt-4">
                <p className="label-dark">Worker Assignments</p>
                <CodeBlock className="text-xs">{JSON.stringify(data.cluster_replay.worker_assignments, null, 2)}</CodeBlock>
              </div>
            </GlassCard>
          </div>
        </>
      ) : (
        <EmptyState title="No replay available" description="Run an execution from the Verification Chat or pick one from history first." />
      )}
    </div>
  );
}

function InfoCard({ icon: Icon, label, value, mono, pill }: { icon: typeof Play; label: string; value: string; mono?: boolean; pill?: boolean }) {
  return (
    <GlassCard className="flex flex-col gap-1.5">
      <span className="flex items-center gap-1.5 text-[11px] font-semibold uppercase tracking-wider text-gray-500">
        <Icon className="h-3.5 w-3.5 text-gray-400" />
        {label}
      </span>
      {pill ? (
        <StatusPill status={value} className="self-start" />
      ) : (
        <span className={`truncate text-sm font-bold ${mono ? 'mono text-accent' : 'text-gray-100'}`}>{value}</span>
      )}
    </GlassCard>
  );
}

function Row({ label, value }: { label: string; value: string }) {
  return (
    <div className="flex items-center justify-between text-sm">
      <span className="text-gray-500">{label}</span>
      <span className="mono font-semibold text-gray-200">{value}</span>
    </div>
  );
}