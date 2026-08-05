import { useEffect } from 'react';
import { useQuery } from '@tanstack/react-query';
import { api } from './lib/api';
import { Sidebar } from './components/Sidebar';
import { TopBar } from './components/TopBar';
import { FirstRunWizard } from './features/wizard/FirstRunWizard';
import { ChatView } from './features/chat/ChatView';
import { MissionControl } from './features/dashboard/MissionControl';
import { ProvidersView } from './features/providers/ProvidersView';
import { CompilerInspectorView } from './features/compiler/CompilerInspectorView';
import { ReplayEngineView } from './features/replay/ReplayEngineView';
import { HistoryView } from './features/history/HistoryView';
import { DiagnosticsView } from './features/diagnostics/DiagnosticsView';
import { personaCanSee, useAppStore, type StudioView } from './store/useAppStore';

export function App() {
  const configured = useAppStore((s) => s.configured);
  const setConfigured = useAppStore((s) => s.setConfigured);
  const activeView = useAppStore((s) => s.activeView);
  const persona = useAppStore((s) => s.persona);
  const setActiveView = useAppStore((s) => s.setActiveView);

  // Apply persona gate: clamp the active view to something the persona may see.
  const effectiveView = personaCanSee(persona, activeView) ? activeView : 'chat';
  useEffect(() => {
    if (effectiveView !== activeView) setActiveView(effectiveView);
  }, [effectiveView, activeView, setActiveView]);

  const { data: health } = useQuery({ queryKey: ['health'], queryFn: api.getHealth });

  if (!configured) {
    return <FirstRunWizard onComplete={() => setConfigured(true)} />;
  }

  return (
    <div className="flex h-screen overflow-hidden bg-ink bg-grid-faint text-gray-100">
      <Sidebar healthVersion={health?.version ?? null} />
      <div className="flex min-w-0 flex-1 flex-col">
        <TopBar />
        <main className="flex-1 overflow-y-auto">
          <div className={effectiveView === 'chat' ? 'h-full p-6 md:p-8' : 'mx-auto max-w-6xl p-6 md:p-8'}>
            {renderView(effectiveView)}
          </div>
        </main>
      </div>
    </div>
  );
}

function renderView(view: StudioView) {
  switch (view) {
    case 'chat':
      return <ChatView />;
    case 'dashboard':
      return <MissionControl />;
    case 'providers':
      return <ProvidersView />;
    case 'compiler':
      return <CompilerInspectorView />;
    case 'replay':
      return <ReplayEngineView />;
    case 'history':
      return <HistoryView />;
    case 'diagnostics':
      return <DiagnosticsView />;
    default:
      return null;
  }
}