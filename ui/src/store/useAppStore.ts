import { create } from 'zustand';
import { persist } from 'zustand/middleware';

export type Persona = 'Beginner' | 'PowerUser' | 'Developer';

export type StudioView = 'chat' | 'dashboard' | 'providers' | 'compiler' | 'replay' | 'history' | 'diagnostics';

export type NavSection = { label: string; views: { id: StudioView; label: string }[] };

export const NAV_SECTIONS: Record<'standard' | 'advanced', NavSection> = {
  standard: {
    label: 'Standard',
    views: [
      { id: 'chat', label: 'Verification Chat' },
      { id: 'dashboard', label: 'Mission Control' },
      { id: 'providers', label: 'Provider Fleet' },
      { id: 'history', label: 'Execution History' },
    ],
  },
  advanced: {
    label: 'Advanced',
    views: [
      { id: 'compiler', label: 'Compiler Inspector' },
      { id: 'replay', label: 'Replay Engine' },
      { id: 'diagnostics', label: 'Platform Diagnostics' },
    ],
  },
};

export const VIEW_TITLES: Record<StudioView, string> = {
  chat: 'Verification Chat',
  dashboard: 'Mission Control',
  providers: 'Provider Fleet',
  compiler: 'Compiler Inspector',
  replay: 'Replay Engine',
  history: 'Execution History',
  diagnostics: 'Platform Diagnostics',
};

interface AppState {
  configured: boolean;
  setConfigured: (value: boolean) => void;
  persona: Persona;
  setPersona: (persona: Persona) => void;
  activeView: StudioView;
  setActiveView: (view: StudioView) => void;
  sessionId: string | null;
  setSessionId: (id: string) => void;
  replayExecutionId: string | null;
  setReplayExecutionId: (id: string | null) => void;
}

export const useAppStore = create<AppState>()(
  persist(
    (set) => ({
      configured: false,
      setConfigured: (value) => set({ configured: value }),
      persona: 'Developer',
      setPersona: (persona) => set({ persona }),
      activeView: 'chat',
      setActiveView: (view) => set({ activeView: view }),
      sessionId: null,
      setSessionId: (sessionId) => set({ sessionId }),
      replayExecutionId: null,
      setReplayExecutionId: (replayExecutionId) => set({ replayExecutionId }),
    }),
    {
      name: 'fusion-studio',
      partialize: (state) => ({ configured: state.configured, persona: state.persona }),
    },
  ),
);

export const personaCanSee = (persona: Persona, view: StudioView): boolean => {
  if (persona === 'Developer' || persona === 'PowerUser') return true;
  return NAV_SECTIONS.standard.views.some((v) => v.id === view);
};