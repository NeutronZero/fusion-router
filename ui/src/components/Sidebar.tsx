import {
  Activity,
  Brain,
  GitBranch,
  History,
  LayoutDashboard,
  MessageSquare,
  Radio,
  Server,
} from 'lucide-react';
import { NAV_SECTIONS, personaCanSee, useAppStore, type StudioView } from '../store/useAppStore';
import { StatusPill, cx } from './ui';

  const VIEW_ICONS: Record<StudioView, typeof LayoutDashboard> = {
  chat: MessageSquare,
  dashboard: LayoutDashboard,
  providers: Server,
  compiler: Brain,
  replay: GitBranch,
  history: History,
  diagnostics: Activity,
};

export function Sidebar({ healthVersion }: { healthVersion: string | null }) {
  const persona = useAppStore((s) => s.persona);
  const activeView = useAppStore((s) => s.activeView);
  const setActiveView = useAppStore((s) => s.setActiveView);

  return (
    <aside className="z-10 flex w-[260px] shrink-0 flex-col border-r border-white/10 bg-[rgba(13,19,33,0.92)] backdrop-blur-2xl">
      {/* Brand */}
      <div className="flex items-center gap-3 px-5 pb-6 pt-6">
        <div className="flex h-9 w-9 items-center justify-center rounded-xl bg-brand-gradient font-bold text-white shadow-glow">
          F
        </div>
        <div className="flex flex-col">
          <span className="text-[15px] font-bold tracking-tight text-gray-100">FusionRouter</span>
          <span className="text-[11px] text-gray-400">Compiler Platform</span>
        </div>
      </div>

      {/* Nav */}
      <nav className="flex flex-1 flex-col gap-1 overflow-y-auto px-3">
        <p className="px-2 pb-1.5 pt-1 text-[10px] font-semibold uppercase tracking-widest text-gray-500">Standard</p>
        {NAV_SECTIONS.standard.views.map((view) => (
          <NavItem key={view.id} id={view.id} label={view.label} active={activeView === view.id} onClick={setActiveView} />
        ))}
        <p className="px-2 pb-1.5 pt-4 text-[10px] font-semibold uppercase tracking-widest text-gray-500">Advanced</p>
        {NAV_SECTIONS.advanced.views.map((view) =>
          personaCanSee(persona, view.id) ? (
            <NavItem key={view.id} id={view.id} label={view.label} active={activeView === view.id} onClick={setActiveView} />
          ) : (
            <div
              key={view.id}
              className="flex cursor-not-allowed select-none items-center gap-3 rounded-lg px-3 py-2.5 text-sm text-gray-600"
              title={`Unlock ${view.label} with the PowerUser or Developer persona`}
            >
              <LockIcon />
              {view.label}
            </div>
          ),
        )}
      </nav>

      {/* Footer */}
      <div className="border-t border-white/10 px-5 py-4">
        <div className="flex items-center gap-2">
          <span className="flex gap-1.5">
            <Radio className="h-3.5 w-3.5 text-emerald-400" />
            <span className="text-xs font-semibold text-emerald-400">{healthVersion ?? 'v0.14'}</span>
          </span>
          <span className="text-[11px] text-gray-500">AF-003 · Zero Bypass</span>
        </div>
        <div className="mt-3">
          <StatusPill status="Engine Ready" className="!bg-white/[0.03]" />
        </div>
      </div>
    </aside>
  );
}

function LockIcon() {
  return (
    <svg className="h-4 w-4 shrink-0" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <rect width="18" height="11" x="3" y="11" rx="2" ry="2" />
      <path d="M7 11V7a5 5 0 0 1 10 0v4" />
    </svg>
  );
}

function NavItem({
  id,
  label,
  active,
  onClick,
}: {
  id: StudioView;
  label: string;
  active: boolean;
  onClick: (view: StudioView) => void;
}) {
  const Icon = VIEW_ICONS[id];
  return (
    <button
      onClick={() => onClick(id)}
      className={cx(
        'flex items-center gap-3 rounded-lg px-3 py-2.5 text-sm font-medium transition-all duration-150',
        active
          ? 'bg-accent font-semibold text-white shadow-glow-sm'
          : 'text-gray-400 hover:bg-white/5 hover:text-gray-100',
      )}
    >
      <Icon className="h-4 w-4 shrink-0" />
      {label}
    </button>
  );
}