import { useState, type ReactNode } from 'react';
import { useMutation } from '@tanstack/react-query';
import { ArrowLeft, ArrowRight, Bot, Check, CheckCircle2, Loader2, Plug, Radar, Rocket } from 'lucide-react';
import { api, PROVIDER_TYPES } from '../../lib/api';
import type { DiscoveredEngine } from '../../lib/apiTypes';
import { StatusPill, cx, toneOf } from '../../components/ui';

const PROVIDER_META: Record<string, { blurb: string; hint: string }> = {
  openrouter: { blurb: 'Gateway to 140+ models with a single API key.', hint: 'https://openrouter.ai/api/v1' },
  anthropic: { blurb: 'Claude family — best-in-class reasoning.', hint: 'https://api.anthropic.com' },
  openai: { blurb: 'GPT-4o and the OpenAI model family.', hint: 'https://api.openai.com/v1' },
  ollama: { blurb: 'Local engine — fully offline, zero cost.', hint: 'http://localhost:11434' },
  lmstudio: { blurb: 'Local engine — your own hardware, offline.', hint: 'http://localhost:1234' },
};

export function FirstRunWizard({ onComplete }: { onComplete: () => void }) {
  const [step, setStep] = useState(1);
  const [selected, setSelected] = useState('ollama');
  const [apiKey, setApiKey] = useState('');
  const [baseUrl, setBaseUrl] = useState(PROVIDER_META.ollama.hint);
  const [testResult, setTestResult] = useState<{ ok: boolean; latencyMs: number; models: string[] } | null>(null);
  const [discovered, setDiscovered] = useState<DiscoveredEngine[]>([]);

  const test = useMutation({
    mutationFn: api.testProvider,
    onSuccess: (data) => setTestResult({ ok: data.status.toLowerCase() === 'healthy', latencyMs: data.latency_ms, models: data.models_discovered }),
    onError: () => setTestResult({ ok: false, latencyMs: 0, models: [] }),
  });

  const discover = useMutation({
    mutationFn: api.discoverWizard,
    onSuccess: (data) => setDiscovered(data.discovered),
  });

  const complete = useMutation({
    mutationFn: api.completeWizard,
    onSuccess: onComplete,
  });

  const meta = PROVIDER_META[selected] ?? { blurb: '', hint: '' };

  const next = () => setStep((s) => Math.min(s + 1, 5));
  const back = () => setStep((s) => Math.max(s - 1, 1));

  return (
    <div className="flex min-h-screen items-center justify-center bg-grid-faint px-4 py-10">
      <div className="w-full max-w-xl animate-fade-in">
        {/* Progress */}
        <div className="mb-6 flex items-center justify-between">
          <div className="flex items-center gap-2">
            <div className="flex h-8 w-8 items-center justify-center rounded-lg bg-brand-gradient font-bold text-white shadow-glow-sm">F</div>
            <span className="text-sm font-semibold text-gray-200">FusionRouter Studio</span>
          </div>
          <span className="mono text-xs text-gray-500">Step {step} / 5</span>
        </div>

        {/* Progress bar */}
        <div className="mb-8 flex gap-1.5">
          {[1, 2, 3, 4, 5].map((s) => (
            <div key={s} className={cx('h-1 flex-1 rounded-full transition-colors duration-300', s <= step ? 'bg-accent shadow-glow-sm' : 'bg-white/10')} />
          ))}
        </div>

        <div className="glass-panel p-8">
          {step === 1 && (
            <StepShell title="Welcome to FusionRouter" subtitle="Choose your primary provider to finish setup in under 5 minutes.">
              <div className="flex flex-col gap-2">
                {PROVIDER_TYPES.map((t) => (
                  <button
                    key={t}
                    onClick={() => setSelected(t)}
                    className={cx(
                      'flex items-start justify-between gap-3 rounded-xl border px-4 py-3 text-left transition-all duration-150',
                      selected === t
                        ? 'border-accent/60 bg-accent/10 shadow-glow-sm'
                        : 'border-white/10 bg-white/[0.02] hover:border-white/25 hover:bg-white/[0.05]',
                    )}
                  >
                    <div>
                      <p className="text-sm font-semibold text-gray-100">{t.toUpperCase()}</p>
                      <p className="mt-0.5 text-xs text-gray-500">{PROVIDER_META[t]?.blurb}</p>
                    </div>
                    {selected === t ? <Check className="h-4 w-4 shrink-0 text-accent" /> : null}
                  </button>
                ))}
              </div>
            </StepShell>
          )}

          {step === 2 && (
            <StepShell title={`Configure ${selected.toUpperCase()}`} subtitle="Keys are encrypted with AES-256-GCM before they ever touch disk.">
              <label className="label-dark">Base URL</label>
              <input className="input-dark" value={baseUrl} onChange={(e) => setBaseUrl(e.target.value)} placeholder={meta.hint} />
              <label className="label-dark mt-4">API Key {selected === 'ollama' || selected === 'lmstudio' ? '(optional — local engines skip auth)' : ''}</label>
              <input className="input-dark" type="password" value={apiKey} onChange={(e) => setApiKey(e.target.value)} placeholder="sk-…" />
            </StepShell>
          )}

          {step === 3 && (
            <StepShell title="Test Connection" subtitle="Validate credentials before saving settings.">
              <div className="flex flex-col items-center gap-4 py-2">
                <button className="btn-primary w-full" onClick={() => test.mutate({ provider_id: selected, base_url: baseUrl || null, api_key: apiKey || null })} disabled={test.isPending}>
                  {test.isPending ? <Loader2 className="h-4 w-4 animate-spin" /> : <Plug className="h-4 w-4" />}
                  {test.isPending ? 'Testing…' : 'Test Connection'}
                </button>
                {testResult && (
                  <div className="w-full animate-slide-up rounded-xl border border-white/10 bg-white/[0.03] p-4">
                    <div className="flex items-center justify-between">
                      <StatusPill status={testResult.ok ? 'Healthy' : 'Failed'} />
                      {testResult.ok && <span className="mono text-sm text-emerald-400">{testResult.latencyMs}ms</span>}
                    </div>
                    {testResult.models.length > 0 && (
                      <div className="mt-3 flex flex-wrap gap-1.5">
                        {testResult.models.map((m) => (
                          <span key={m} className="mono rounded-md border border-white/10 bg-black/30 px-2 py-0.5 text-[11px] text-gray-300">{m}</span>
                        ))}
                      </div>
                    )}
                  </div>
                )}
              </div>
            </StepShell>
          )}

          {step === 4 && (
            <StepShell title="Local Model Discovery" subtitle="Probing localhost for Ollama and LM Studio engines.">
              <button className="btn-secondary w-full" onClick={() => discover.mutate()} disabled={discover.isPending}>
                {discover.isPending ? <Loader2 className="h-4 w-4 animate-spin" /> : <Radar className="h-4 w-4" />}
                {discover.isPending ? 'Scanning…' : 'Scan Local Engines'}
              </button>
              {discovered.length > 0 && (
                <div className="mt-4 flex flex-col gap-2">
                  {discovered.map((engine) => (
                    <div key={engine.endpoint} className="rounded-xl border border-white/10 bg-white/[0.02] p-3.5">
                      <div className="flex items-center justify-between">
                        <span className="text-sm font-semibold text-gray-100">{engine.name}</span>
                        <span className={cx('text-xs font-semibold', toneOf(engine.status) === 'success' ? 'text-emerald-400' : 'text-amber-400')}>
                          {engine.status}
                        </span>
                      </div>
                      <p className="mono mt-1 text-[11px] text-gray-500">{engine.endpoint}</p>
                      <div className="mt-2 flex flex-wrap gap-1.5">
                        {engine.models.map((m) => (
                          <span key={m} className="mono rounded-md border border-white/10 bg-black/30 px-2 py-0.5 text-[11px] text-gray-300">{m}</span>
                        ))}
                      </div>
                    </div>
                  ))}
                </div>
              )}
            </StepShell>
          )}

          {step === 5 && (
            <StepShell title="Setup Complete!" subtitle="FusionRouter is configured and ready for compiler-driven AI orchestration.">
              <div className="flex flex-col items-center gap-3 py-4 text-center">
                <div className="flex h-16 w-16 items-center justify-center rounded-2xl bg-success-soft shadow-glow-emerald">
                  <CheckCircle2 className="h-8 w-8 text-emerald-400" />
                </div>
                <p className="text-sm text-gray-400">
                  Default provider: <span className="mono font-semibold text-accent">{selected.toUpperCase()}</span>
                </p>
              </div>
            </StepShell>
          )}

          {/* Footer nav */}
          <div className="mt-8 flex items-center justify-between border-t border-white/10 pt-5">
            <button className="btn-secondary" onClick={back} disabled={step === 1 || complete.isPending}>
              <ArrowLeft className="h-4 w-4" /> Back
            </button>
            {step < 5 ? (
              <button className="btn-primary" onClick={next} disabled={step === 3 && (!testResult || test.isPending)}>
                Next <ArrowRight className="h-4 w-4" />
              </button>
            ) : (
              <button className="btn-primary" onClick={() => complete.mutate(selected)} disabled={complete.isPending}>
                {complete.isPending ? <Loader2 className="h-4 w-4 animate-spin" /> : <Rocket className="h-4 w-4" />}
                Launch Studio
              </button>
            )}
          </div>
        </div>

        <p className="mt-6 flex items-center justify-center gap-2 text-center text-xs text-gray-600">
          <Bot className="h-3.5 w-3.5" /> Every prompt executes through the Planner → Compiler → Scheduler → Runtime pipeline.
        </p>
      </div>
    </div>
  );
}

function StepShell({ title, subtitle, children }: { title: string; subtitle: string; children: ReactNode }) {
  return (
    <div className="animate-fade-in">
      <h1 className="text-2xl font-extrabold tracking-tight text-gray-50">{title}</h1>
      <p className="mt-1 text-sm text-gray-400">{subtitle}</p>
      <div className="mt-6">{children}</div>
    </div>
  );
}