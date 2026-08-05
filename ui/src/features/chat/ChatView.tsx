import { useEffect, useRef, useState } from 'react';
import { useMutation } from '@tanstack/react-query';
import { Bot, Copy, Send, Sparkles } from 'lucide-react';
import { api } from '../../lib/api';
import { StudioChatResponse } from '../../lib/apiTypes';
import { useAppStore } from '../../store/useAppStore';
import { StatusPill, cx } from '../../components/ui';

interface ChatMessage {
  id: string;
  role: 'user' | 'assistant';
  content: string;
  response?: StudioChatResponse;
  pending?: boolean;
  error?: boolean;
}

const WELCOME: ChatMessage = {
  id: 'welcome',
  role: 'assistant',
  content:
    'FusionRouter engine is ready. Send any prompt and it will be compiled through Planner → Compiler → Scheduler → Runtime, then traced with execution, timeline and compiler-pass details.',
};

let msgSeq = 0;
const nextId = () => `msg_${Date.now()}_${msgSeq++}`;

export function ChatView() {
  const sessionId = useAppStore((s) => s.sessionId);
  const setSessionId = useAppStore((s) => s.setSessionId);
  const [messages, setMessages] = useState<ChatMessage[]>([WELCOME]);
  const [prompt, setPrompt] = useState('');
  const scrollRef = useRef<HTMLDivElement>(null);

  const chat = useMutation({
    mutationFn: api.chat,
    onSuccess: (data) => {
      setSessionId(data.session_id);
      setMessages((prev) => [
        ...prev,
        { id: nextId(), role: 'assistant', content: data.reply, response: data },
      ]);
    },
    onError: () => {
      setMessages((prev) => [
        ...prev,
        {
          id: nextId(),
          role: 'assistant',
          content:
            'Request failed. Make sure the FusionStudio server is running on port 8080 (or start `npm run dev` inside `apps/fusion-server`).',
          error: true,
        },
      ]);
    },
  });

  const submit = () => {
    const text = prompt.trim();
    if (!text || chat.isPending) return;
    setMessages((prev) => [
      ...prev,
      { id: nextId(), role: 'user', content: text },
      { id: nextId(), role: 'assistant', content: 'Compiling pipeline (Planner → Compiler → Runtime)...', pending: true },
    ]);
    setPrompt('');
    chat.mutate({ prompt: text, session_id: sessionId, provider_preference: null });
  };

  useEffect(() => {
    scrollRef.current?.scrollTo({ top: scrollRef.current.scrollHeight, behavior: 'smooth' });
  }, [messages]);

  return (
    <div className="mx-auto flex h-full w-full max-w-3xl flex-col gap-4">
      {/* Hero */}
      <div className="hero-banner text-center">
        <h1 className="text-3xl font-extrabold tracking-tight">
          Compile <span className="text-gradient">&amp;</span> Execute AI Workflows
        </h1>
        <p className="mt-1.5 text-sm text-gray-400">
          Every prompt runs through the deterministic Planner → Compiler → Scheduler → Runtime pipeline.
        </p>
      </div>

      {/* Messages */}
      <div ref={scrollRef} className="glass-panel flex flex-1 flex-col gap-3 overflow-y-auto p-5">
        {messages.map((m) => (
          <MessageBubble key={m.id} message={m} />
        ))}
      </div>

      {/* Input */}
      <form
        onSubmit={(e) => {
          e.preventDefault();
          submit();
        }}
        className="flex items-center gap-2.5 rounded-2xl border border-white/10 bg-white/[0.03] p-2.5 backdrop-blur-xl"
      >
        <input
          value={prompt}
          onChange={(e) => setPrompt(e.target.value)}
          placeholder="Send a prompt through the compiler pipeline…"
          className="flex-1 bg-transparent px-3.5 py-2.5 text-sm text-gray-100 placeholder-gray-500 outline-none"
        />
        <button type="submit" className="btn-primary" disabled={chat.isPending}>
          {chat.isPending ? <Bot className="h-4 w-4 animate-pulse" /> : <Send className="h-4 w-4 text-white" />}
          {chat.isPending ? 'Compiling…' : 'Compile & Send'}
        </button>
      </form>
    </div>
  );
}

function MessageBubble({ message }: { message: ChatMessage }) {
  if (message.role === 'user') {
    return (
      <div className="flex justify-end">
        <div className="max-w-[80%] rounded-2xl rounded-br-sm bg-accent px-4 py-3 text-sm text-white shadow-glow-sm">
          {message.content}
        </div>
      </div>
    );
  }

  if (message.pending) {
    return (
      <div className="flex items-start gap-3">
        <Avatar />
        <div className="flex items-center gap-2 rounded-2xl rounded-tl-sm border border-white/10 bg-black/40 px-4 py-3 text-sm text-gray-300">
          <Sparkles className="h-4 w-4 animate-pulse text-accent" />
          {message.content}
        </div>
      </div>
    );
  }

  return (
    <div className="flex items-start gap-3">
      <Avatar />
      <div className={cx('w-full max-w-full', message.error && 'opacity-90')}>
        <div
          className={cx(
            'rounded-2xl rounded-tl-sm border px-4 py-3 text-sm leading-relaxed',
            message.error ? 'border-danger-border bg-danger-soft text-rose-200' : 'border-white/10 bg-black/40 text-gray-200',
          )}
        >
          <strong className="block text-gray-100">{message.content}</strong>
          {message.response && <ResponseDetails response={message.response} />}
        </div>
      </div>
    </div>
  );
}

function Avatar() {
  return (
    <div className="flex h-8 w-8 shrink-0 items-center justify-center rounded-lg bg-brand-gradient shadow-glow-sm">
      <Bot className="h-4 w-4 text-white" />
    </div>
  );
}

function ResponseDetails({ response }: { response: StudioChatResponse }) {
  const badge = response.execution_badge;
  return (
    <div className="mt-3 space-y-3 border-t border-white/10 pt-3">
      {/* Execution badge */}
      <div className="flex flex-wrap items-center gap-2">
        <CodePill label="exec" value={badge.execution_id.slice(0, 8)} />
        <CodePill label="graph" value={badge.graph_id.slice(0, 8)} />
        <CodePill label="passes" value={String(badge.passes_executed)} />
        <StatusPill status={badge.status} />
        <span className="text-xs text-gray-500">provider: {response.provider}</span>
      </div>

      {/* Timeline */}
      <div className="grid grid-cols-5 gap-1.5">
        {response.timeline.map((step) => (
          <div key={step.stage} className="rounded-lg border border-white/10 bg-white/[0.03] px-2 py-1.5 text-center">
            <p className="text-[10px] font-semibold uppercase tracking-wide text-gray-500">{step.stage}</p>
            <p className="mono mt-0.5 text-sm font-semibold text-gray-200">{step.duration_ms}ms</p>
          </div>
        ))}
      </div>

      <button className="btn-ghost !px-2 !py-1 text-xs" onClick={() => navigator.clipboard.writeText(badge.execution_id)}>
        <Copy className="h-3.5 w-3.5" /> Copy execution ID
      </button>
    </div>
  );
}

function CodePill({ label, value }: { label: string; value: string }) {
  return (
    <span className="inline-flex items-center gap-1.5 rounded-md border border-white/10 bg-black/30 px-2 py-0.5 text-[11px] text-gray-400">
      <span className="text-gray-500">{label}</span>
      <span className="mono font-semibold text-accent">{value}</span>
    </span>
  );
}