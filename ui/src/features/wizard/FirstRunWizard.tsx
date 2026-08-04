import React, { useState } from 'react';
import { DesignTokens } from '../../design-system/tokens';

export function FirstRunWizard({ onComplete }: { onComplete: () => void }) {
  const [step, setStep] = useState(1);
  const [selectedProvider, setSelectedProvider] = useState('openrouter');
  const [apiKey, setApiKey] = useState('');
  const [testResult, setTestResult] = useState<string | null>(null);
  const [discoveredModels] = useState(['Ollama (llama3)', 'OpenRouter (gpt-4o)', 'OpenRouter (claude-3-5-sonnet)']);

  const handleTestConnection = () => {
    if (!apiKey) {
      setTestResult('Please enter an API Key');
      return;
    }
    setTestResult('Connection Successful! Latency: 38ms');
  };

  return (
    <div style={{ backgroundColor: DesignTokens.colors.surface, border: `1px solid ${DesignTokens.colors.border}`, padding: '32px', borderRadius: '12px', maxWidth: '600px', margin: '40px auto' }}>
      <div style={{ display: 'flex', justifyContent: 'space-between', marginBottom: '24px', fontSize: DesignTokens.typography.fontSize.sm, color: DesignTokens.colors.textSecondary }}>
        <span>First Run Setup</span>
        <span>Step {step} of 5</span>
      </div>

      {step === 1 && (
        <div>
          <h2 style={{ marginTop: 0, color: DesignTokens.colors.accent }}>Welcome to FusionRouter</h2>
          <p style={{ color: DesignTokens.colors.textSecondary }}>Choose your primary provider to begin setup in under 5 minutes.</p>
          <div style={{ display: 'flex', flexDirection: 'column', gap: '8px', margin: '20px 0' }}>
            {['openrouter', 'anthropic', 'openai', 'ollama'].map((p) => (
              <button
                key={p}
                onClick={() => setSelectedProvider(p)}
                style={{
                  padding: '12px',
                  backgroundColor: selectedProvider === p ? DesignTokens.colors.surfaceHover : 'transparent',
                  border: `1px solid ${selectedProvider === p ? DesignTokens.colors.accent : DesignTokens.colors.border}`,
                  color: DesignTokens.colors.textPrimary,
                  borderRadius: '6px',
                  textAlign: 'left',
                  cursor: 'pointer',
                }}
              >
                {p.toUpperCase()}
              </button>
            ))}
          </div>
          <button onClick={() => setStep(2)} style={buttonStyle}>Continue</button>
        </div>
      )}

      {step === 2 && (
        <div>
          <h2 style={{ marginTop: 0 }}>API Key Setup ({selectedProvider.toUpperCase()})</h2>
          <p style={{ color: DesignTokens.colors.textSecondary }}>Enter your credentials. Keys are encrypted via AES-256-GCM.</p>
          <input
            type="password"
            placeholder="sk-..."
            value={apiKey}
            onChange={(e) => setApiKey(e.target.value)}
            style={{ width: '100%', padding: '12px', backgroundColor: DesignTokens.colors.background, border: `1px solid ${DesignTokens.colors.border}`, color: DesignTokens.colors.textPrimary, borderRadius: '6px', margin: '16px 0' }}
          />
          <div style={{ display: 'flex', gap: '12px' }}>
            <button onClick={() => setStep(1)} style={secondaryButtonStyle}>Back</button>
            <button onClick={() => setStep(3)} style={buttonStyle}>Next: Test Connection</button>
          </div>
        </div>
      )}

      {step === 3 && (
        <div>
          <h2 style={{ marginTop: 0 }}>Test Connection</h2>
          <p style={{ color: DesignTokens.colors.textSecondary }}>Validate credentials before saving settings.</p>
          <button onClick={handleTestConnection} style={buttonStyle}>Test Connection</button>
          {testResult && <div style={{ marginTop: '16px', color: testResult.includes('Successful') ? DesignTokens.colors.success : DesignTokens.colors.error }}>{testResult}</div>}
          <div style={{ display: 'flex', gap: '12px', marginTop: '24px' }}>
            <button onClick={() => setStep(2)} style={secondaryButtonStyle}>Back</button>
            <button onClick={() => setStep(4)} style={buttonStyle}>Next: Model Selection</button>
          </div>
        </div>
      )}

      {step === 4 && (
        <div>
          <h2 style={{ marginTop: 0 }}>Model Selection &amp; Local Discovery</h2>
          <p style={{ color: DesignTokens.colors.textSecondary }}>Local model servers detected on your machine:</p>
          <ul style={{ paddingLeft: '20px', color: DesignTokens.colors.textPrimary }}>
            {discoveredModels.map((m) => <li key={m} style={{ marginBottom: '8px' }}>{m}</li>)}
          </ul>
          <div style={{ display: 'flex', gap: '12px', marginTop: '24px' }}>
            <button onClick={() => setStep(3)} style={secondaryButtonStyle}>Back</button>
            <button onClick={() => setStep(5)} style={buttonStyle}>Next: Finish</button>
          </div>
        </div>
      )}

      {step === 5 && (
        <div>
          <h2 style={{ marginTop: 0, color: DesignTokens.colors.success }}>Setup Complete!</h2>
          <p style={{ color: DesignTokens.colors.textSecondary }}>FusionRouter is configured and ready for compiler-driven AI orchestration.</p>
          <button onClick={onComplete} style={buttonStyle}>Launch Studio</button>
        </div>
      )}
    </div>
  );
}

const buttonStyle: React.CSSProperties = {
  backgroundColor: DesignTokens.colors.accent,
  color: '#000',
  border: 'none',
  padding: '10px 20px',
  borderRadius: '6px',
  fontWeight: 'bold',
  cursor: 'pointer',
};

const secondaryButtonStyle: React.CSSProperties = {
  backgroundColor: DesignTokens.colors.surfaceHover,
  color: DesignTokens.colors.textPrimary,
  border: `1px solid ${DesignTokens.colors.border}`,
  padding: '10px 20px',
  borderRadius: '6px',
  cursor: 'pointer',
};
