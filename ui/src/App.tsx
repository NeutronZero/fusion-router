import React, { useState } from 'react';
import { DesignTokens } from './design-system/tokens';

export type Persona = 'Beginner' | 'PowerUser' | 'Developer';

export function App() {
  const [persona, setPersona] = useState<Persona>('Developer');
  const [activeTab, setActiveTab] = useState<'dashboard' | 'chat' | 'providers' | 'compiler' | 'diagnostics'>('dashboard');

  return (
    <div style={{ backgroundColor: DesignTokens.colors.background, color: DesignTokens.colors.textPrimary, minHeight: '100vh', fontFamily: DesignTokens.typography.fontFamily }}>
      {/* Header Bar */}
      <header style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', padding: '16px 24px', borderBottom: `1px solid ${DesignTokens.colors.border}`, backgroundColor: DesignTokens.colors.surface }}>
        <div style={{ display: 'flex', alignItems: 'center', gap: '12px' }}>
          <h1 style={{ margin: 0, fontSize: DesignTokens.typography.fontSize.xl, fontWeight: 'bold', color: DesignTokens.colors.accent }}>FusionRouter Studio</h1>
          <span style={{ fontSize: '11px', backgroundColor: DesignTokens.colors.surfaceHover, padding: '2px 8px', borderRadius: '12px', border: `1px solid ${DesignTokens.colors.border}` }}>v0.14.0 (AF-003)</span>
        </div>

        {/* Persona Switcher */}
        <div style={{ display: 'flex', alignItems: 'center', gap: '8px' }}>
          <span style={{ fontSize: DesignTokens.typography.fontSize.sm, color: DesignTokens.colors.textSecondary }}>Persona:</span>
          {(['Beginner', 'PowerUser', 'Developer'] as Persona[]).map((p) => (
            <button
              key={p}
              onClick={() => setPersona(p)}
              style={{
                backgroundColor: persona === p ? DesignTokens.colors.accent : DesignTokens.colors.surfaceHover,
                color: persona === p ? '#000' : DesignTokens.colors.textPrimary,
                border: `1px solid ${DesignTokens.colors.border}`,
                padding: '4px 12px',
                borderRadius: '4px',
                cursor: 'pointer',
                fontSize: DesignTokens.typography.fontSize.sm,
              }}
            >
              {p}
            </button>
          ))}
        </div>
      </header>

      {/* Main Body Layout */}
      <div style={{ display: 'flex', minHeight: 'calc(100vh - 65px)' }}>
        {/* Navigation Sidebar */}
        <nav style={{ width: '220px', borderRight: `1px solid ${DesignTokens.colors.border}`, backgroundColor: DesignTokens.colors.surface, padding: '16px 8px' }}>
          <div style={{ fontSize: '11px', color: DesignTokens.colors.textSecondary, textTransform: 'uppercase', marginBottom: '8px', paddingLeft: '8px' }}>Standard</div>
          <button onClick={() => setActiveTab('dashboard')} style={navButtonStyle(activeTab === 'dashboard')}>Dashboard</button>
          <button onClick={() => setActiveTab('chat')} style={navButtonStyle(activeTab === 'chat')}>Chat Verification</button>
          <button onClick={() => setActiveTab('providers')} style={navButtonStyle(activeTab === 'providers')}>Providers</button>

          {(persona === 'PowerUser' || persona === 'Developer') && (
            <>
              <div style={{ fontSize: '11px', color: DesignTokens.colors.textSecondary, textTransform: 'uppercase', margin: '16px 0 8px 0', paddingLeft: '8px' }}>Advanced</div>
              <button onClick={() => setActiveTab('compiler')} style={navButtonStyle(activeTab === 'compiler')}>Compiler Inspector</button>
              <button onClick={() => setActiveTab('diagnostics')} style={navButtonStyle(activeTab === 'diagnostics')}>System Diagnostics</button>
            </>
          )}
        </nav>

        {/* Content Area */}
        <main style={{ flex: 1, padding: '24px' }}>
          {activeTab === 'dashboard' && (
            <div>
              <h2 style={{ marginTop: 0 }}>Platform Overview</h2>
              <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fit, minmax(200px, 1fr))', gap: '16px', marginTop: '16px' }}>
                <Card title="System Health" value="Healthy (Ready)" color={DesignTokens.colors.success} />
                <Card title="Connected Providers" value="3 Active" color={DesignTokens.colors.accent} />
                <Card title="Compiler Passes" value="9 Passes (0.2ms)" color={DesignTokens.colors.textPrimary} />
                <Card title="Architecture Laws" value="17 Active (AF-003)" color={DesignTokens.colors.textSecondary} />
              </div>
            </div>
          )}

          {activeTab === 'chat' && (
            <div>
              <h2 style={{ marginTop: 0 }}>Studio Verification Chat</h2>
              <p style={{ color: DesignTokens.colors.textSecondary }}>Chat interactions execute strictly through the <code>Planner -&gt; Compiler -&gt; Runtime</code> pipeline (Law 1 &amp; Law 11).</p>
            </div>
          )}

          {activeTab === 'providers' && (
            <div>
              <h2 style={{ marginTop: 0 }}>Provider Manager</h2>
              <p style={{ color: DesignTokens.colors.textSecondary }}>Configure OpenRouter, Zen, and local Ollama model servers with AES-256-GCM encrypted credentials.</p>
            </div>
          )}

          {activeTab === 'compiler' && (
            <div>
              <h2 style={{ marginTop: 0 }}>Compiler Inspector &amp; Explain Route</h2>
              <p style={{ color: DesignTokens.colors.textSecondary }}>Multi-dimensional routing score breakdown: Capability, Budget, Latency, Health, Policy.</p>
            </div>
          )}

          {activeTab === 'diagnostics' && (
            <div>
              <h2 style={{ marginTop: 0 }}>1-Click System Check</h2>
              <div style={{ backgroundColor: DesignTokens.colors.surface, padding: '16px', borderRadius: '8px', border: `1px solid ${DesignTokens.colors.border}` }}>
                <div style={{ display: 'flex', justifyContent: 'space-between', padding: '8px 0', borderBottom: `1px solid ${DesignTokens.colors.border}` }}>
                  <span>API Gateway Gateway</span>
                  <span style={{ color: DesignTokens.colors.success }}>✓ Healthy (1ms)</span>
                </div>
                <div style={{ display: 'flex', justifyContent: 'space-between', padding: '8px 0', borderBottom: `1px solid ${DesignTokens.colors.border}` }}>
                  <span>SQLite Database (fusion_data.db)</span>
                  <span style={{ color: DesignTokens.colors.success }}>✓ Healthy (2ms)</span>
                </div>
                <div style={{ display: 'flex', justifyContent: 'space-between', padding: '8px 0' }}>
                  <span>Provider Connection Test</span>
                  <span style={{ color: DesignTokens.colors.success }}>✓ Healthy (38ms)</span>
                </div>
              </div>
            </div>
          )}
        </main>
      </div>
    </div>
  );
}

function navButtonStyle(active: boolean): React.CSSProperties {
  return {
    display: 'block',
    width: '100%',
    textAlign: 'left',
    padding: '10px 12px',
    marginBottom: '4px',
    backgroundColor: active ? DesignTokens.colors.surfaceHover : 'transparent',
    color: active ? DesignTokens.colors.accent : DesignTokens.colors.textPrimary,
    border: 'none',
    borderRadius: '6px',
    cursor: 'pointer',
    fontWeight: active ? 'bold' : 'normal',
    fontSize: DesignTokens.typography.fontSize.base,
  };
}

function Card({ title, value, color }: { title: string; value: string; color: string }) {
  return (
    <div style={{ backgroundColor: DesignTokens.colors.surface, border: `1px solid ${DesignTokens.colors.border}`, padding: '16px', borderRadius: '8px' }}>
      <div style={{ fontSize: DesignTokens.typography.fontSize.sm, color: DesignTokens.colors.textSecondary, marginBottom: '8px' }}>{title}</div>
      <div style={{ fontSize: DesignTokens.typography.fontSize.lg, fontWeight: 'bold', color }}>{value}</div>
    </div>
  );
}
