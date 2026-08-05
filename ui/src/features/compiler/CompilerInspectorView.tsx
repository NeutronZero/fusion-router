import { useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import { Brain, Route } from 'lucide-react';
import { api } from '../../lib/api';
import { CodeBlock, ErrorBanner, GlassCard, PageHeader, Skeleton } from '../../components/ui';

const PIPELINE_PASSES = [
  'Validation',
  'Capability Resolution',
  'Constraint Solver',
  'Constant Folding',
  'Dead Node Elimination',
  'Node Fusion',
  'Retry Injection',
  'Fallback Injection',
  'Scheduling Hints',
];

const PROVIDERS = ['ollama', 'openrouter', 'anthropic', 'openai'];

export function CompilerInspectorView() {
  const [provider, setProvider] = useState('openrouter');
  const { data, isLoading, error, refetch } = useQuery({
    queryKey: ['explain', provider],
    queryFn: () => api.explainRoute(provider),
  });

  return (
    <div className="flex flex-col gap-6">
      <PageHeader
        title="Compiler Inspector"
        subtitle="Multi-dimensional routing score breakdown: Capability, Budget, Latency, Health and Policy."
        right={
          <select className="input-dark !w-48" value={provider} onChange={(e) => setProvider(e.target.value)}>
            {PROVIDERS.map((p) => (
              <option key={p} value={p}>
                {p}
              </option>
            ))}
          </select>
        }
      />

      <GlassCard>
        <div className="mb-4 flex items-center gap-2">
          <Brain className="h-4 w-4 text-accent" />
          <h3 className="text-sm font-bold text-gray-100">Pass Transformation Sequence</h3>
        </div>
        <div className="flex flex-wrap items-center gap-2">
          {PIPELINE_PASSES.map((pass, i) => (
            <span key={pass} className="flex items-center gap-2">
              <span className="flex items-center gap-1.5 rounded-lg border border-white/10 bg-white/[0.03] px-2.5 py-1.5 text-xs text-gray-300">
                <span className="mono text-gray-500">{String(i + 1).padStart(2, '0')}</span>
                {pass}
              </span>
              {i < PIPELINE_PASSES.length - 1 && <span className="text-gray-600">→</span>}
            </span>
          ))}
        </div>
      </GlassCard>

      {error && <ErrorBanner message="Could not compute route score for this provider." onRetry={() => refetch()} />}

      {isLoading && !data ? (
        <GlassCard className="flex flex-col gap-3">
          {Array.from({ length: 6 }).map((_, i) => (
            <Skeleton key={i} className="h-8" />
          ))}
        </GlassCard>
      ) : data ? (
        <RouteScoreCard scores={data} />
      ) : null}
    </div>
  );
}

function RouteScoreCard({ scores }: { scores: Record<string, unknown> }) {
  const entries = Object.entries(scores);
  return (
    <GlassCard>
      <div className="mb-4 flex items-center gap-2">
        <Route className="h-4 w-4 text-accent" />
        <h3 className="text-sm font-bold text-gray-100">Route Explanation</h3>
      </div>
      {entries.length > 0 ? (
        <div className="grid grid-cols-2 gap-3 md:grid-cols-3">
          {entries.map(([key, value]) => (
            <ScoreCell key={key} label={key} value={value} />
          ))}
        </div>
      ) : (
        <p className="text-sm text-gray-500">No score fields returned for this provider.</p>
      )}
      <div className="mt-4">
        <CodeBlock className="text-xs">{JSON.stringify(scores, null, 2)}</CodeBlock>
      </div>
    </GlassCard>
  );
}

function ScoreCell({ label, value }: { label: string; value: unknown }) {
  const isNumber = typeof value === 'number';
  const display = isNumber && !Number.isInteger(value) ? value.toFixed(3) : String(value);
  return (
    <div className="rounded-xl border border-white/10 bg-white/[0.02] p-3.5">
      <p className="text-[11px] font-semibold uppercase tracking-wide text-gray-500">{label}</p>
      <p className={`mono mt-1.5 break-words text-sm font-bold ${isNumber ? 'text-accent' : 'text-gray-200'}`}>{display}</p>
    </div>
  );
}