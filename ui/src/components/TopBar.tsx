import { ShieldCheck } from 'lucide-react';
import { useQuery } from '@tanstack/react-query';
import { api } from '../lib/api';
import { VIEW_TITLES, useAppStore, type Persona } from '../store/useAppStore';
import { cx } from './ui';

const PERSONAS: Persona[] = ['Beginner', 'PowerUser', 'Developer'];

export function TopBar() {
  const activeView = useAppStore((s) => s.activeView);
  const persona = useAppStore((s) => s.persona);
  const setPersona = useAppStore((s) => s.setPersona);

  const { data: health } = useQuery({ queryKey: ['health'], queryFn: api.getHealth });

  return (
    <header className="flex h-16 shrink-0 items-center justify-between gap-4 border-b border-white/10 bg-[rgba(13,19,33,0.4)] px-8 backdrop-blur-xl">
      <div className="flex min-w-0 items-center gap-3">
        <h2 className="truncate text-lg font-bold text-gray-100">{VIEW_TITLES[activeView]}</h2>
        {health && (
          <span className="hidden items-center gap-1.5 rounded-full bg-white/[0.06] px-3 py-1 text-xs text-gray-400 md:inline-flex">
            <ShieldCheck className="h-3.5 w-3.5 text-emerald-400" />
            v{health.version} · {health.laws_active} laws · {health.status}
          </span>
        )}
      </div>

      {/* Persona segmented control */}
      <div className="flex items-center gap-1 rounded-xl border border-white/10 bg-black/20 p-1">
        {PERSONAS.map((p) => (
          <button
            key={p}
            onClick={() => setPersona(p)}
            className={cx(
              'rounded-lg px-3 py-1.5 text-xs font-semibold transition-all duration-150',
              persona === p ? 'bg-accent text-white shadow-glow-sm' : 'text-gray-400 hover:text-gray-100',
            )}
          >
            {p}
          </button>
        ))}
      </div>
    </header>
  );
}