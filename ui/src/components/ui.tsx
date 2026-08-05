import type { ReactNode } from 'react';
import {
  AlertTriangle,
  CheckCircle2,
  Loader2,
  RefreshCcw,
  Wifi,
  XCircle,
} from 'lucide-react';

export function cx(...parts: Array<string | false | null | undefined>): string {
  return parts.filter(Boolean).join(' ');
}

/* ---------------------------------- Card ---------------------------------- */

export function GlassCard({
  children,
  className = '',
  hover = false,
}: {
  children: ReactNode;
  className?: string;
  hover?: boolean;
}) {
  return <div className={cx('glass-panel p-5', hover && 'glass-panel-hover', className)}>{children}</div>;
}

/* ---------------------------------- Badge --------------------------------- */

export type Tone = 'success' | 'info' | 'warning' | 'danger' | 'neutral';

export function toneOf(status: string): Tone {
  const s = status.toLowerCase();
  if (['healthy', 'ready', 'completed', 'passed', 'serving', 'active', 'detected', 'ok', 'up'].includes(s)) return 'success';
  if (['error', 'failed', 'down', 'unavailable', 'unhealthy'].includes(s)) return 'danger';
  if (['running', 'starting', 'compiling', 'planning', 'executing', 'pending', 'loading'].includes(s)) return 'info';
  if (['warning', 'degraded', 'degrading'].includes(s)) return 'warning';
  return 'neutral';
}

const toneClasses: Record<Tone, string> = {
  success: 'bg-success-soft text-emerald-400 border-success-border',
  info: 'bg-info-soft text-sky-400 border-info-border',
  warning: 'bg-warning-soft text-amber-400 border-warning-border',
  danger: 'bg-danger-soft text-rose-400 border-danger-border',
  neutral: 'bg-white/5 text-gray-300 border-white/10',
};

export function StatusPill({ status, className = '' }: { status: string; className?: string }) {
  const tone = toneOf(status);
  return (
    <span
      className={cx(
        'inline-flex items-center gap-1.5 whitespace-nowrap rounded-full border px-2.5 py-0.5 text-xs font-semibold',
        toneClasses[tone],
        className,
      )}
    >
      <StatusDot tone={tone} />
      {status}
    </span>
  );
}

export function StatusDot({ tone }: { tone: Tone }) {
  const map: Record<Tone, string> = {
    success: 'bg-emerald-400 shadow-[0_0_8px_rgba(16,185,129,0.9)]',
    info: 'bg-sky-400 shadow-[0_0_8px_rgba(56,189,248,0.9)]',
    warning: 'bg-amber-400 shadow-[0_0_8px_rgba(245,158,11,0.9)]',
    danger: 'bg-rose-400 shadow-[0_0_8px_rgba(244,63,94,0.9)]',
    neutral: 'bg-gray-400',
  };
  return <span className={cx('inline-block h-1.5 w-1.5 shrink-0 rounded-full', map[tone])} />;
}

export function Badge({
  children,
  tone = 'neutral',
  className = '',
}: {
  children: ReactNode;
  tone?: Tone;
  className?: string;
}) {
  return (
    <span className={cx('inline-flex items-center gap-1 rounded-md border px-2 py-0.5 text-[11px] font-medium', toneClasses[tone], className)}>
      {children}
    </span>
  );
}

/* -------------------------------- Stat card ------------------------------- */

export function StatCard({
  label,
  value,
  tone = 'neutral',
  sub,
  mono = true,
}: {
  label: string;
  value: string | number;
  tone?: Tone;
  sub?: string;
  mono?: boolean;
}) {
  const valueColor: Record<Tone, string> = {
    success: 'text-emerald-400',
    info: 'text-sky-400',
    warning: 'text-amber-400',
    danger: 'text-rose-400',
    neutral: 'text-gray-100',
  };
  return (
    <GlassCard hover className="flex flex-col gap-1.5">
      <span className="text-[11px] font-semibold uppercase tracking-wider text-gray-400">{label}</span>
      <span className={cx('text-3xl font-bold', mono && 'font-mono text-2xl', valueColor[tone])}>{value}</span>
      {sub && <span className="text-xs text-gray-500">{sub}</span>}
    </GlassCard>
  );
}

/* --------------------------------- Header --------------------------------- */

export function PageHeader({ title, subtitle, right }: { title: string; subtitle?: string; right?: ReactNode }) {
  return (
    <div className="flex flex-wrap items-start justify-between gap-4">
      <div>
        <h1 className="text-xl font-bold tracking-tight text-gray-100">{title}</h1>
        {subtitle && <p className="mt-1 max-w-2xl text-sm text-gray-400">{subtitle}</p>}
      </div>
      {right && <div className="flex items-center gap-2">{right}</div>}
    </div>
  );
}

/* ------------------------------ Skeleton etc ------------------------------ */

export function Skeleton({ className = '' }: { className?: string }) {
  return <div className={cx('animate-pulse rounded-lg bg-white/[0.06]', className)} />;
}

export function Spinner({ className = '' }: { className?: string }) {
  return <Loader2 className={cx('h-4 w-4 animate-spin text-accent', className)} />;
}

export function EmptyState({
  title,
  description,
  action,
}: {
  title: string;
  description?: string;
  action?: ReactNode;
}) {
  return (
    <div className="flex flex-col items-center justify-center gap-3 rounded-2xl border border-dashed border-white/10 py-16 text-center">
      <div className="flex h-12 w-12 items-center justify-center rounded-2xl bg-white/5 text-gray-400">
        <Wifi className="h-6 w-6" />
      </div>
      <div>
        <p className="font-semibold text-gray-200">{title}</p>
        {description && <p className="mt-1 max-w-sm text-sm text-gray-500">{description}</p>}
      </div>
      {action}
    </div>
  );
}

export function ErrorBanner({ message, onRetry }: { message: string; onRetry?: () => void }) {
  return (
    <div className="flex items-center justify-between gap-4 rounded-xl border border-danger-border bg-danger-soft px-4 py-3">
      <div className="flex items-center gap-3">
        {message.toLowerCase().includes('offline') ? (
          <AlertTriangle className="h-5 w-5 shrink-0 text-amber-400" />
        ) : (
          <XCircle className="h-5 w-5 shrink-0 text-rose-400" />
        )}
        <span className="text-sm text-gray-200">{message}</span>
      </div>
      {onRetry && (
        <button className="btn-secondary !py-1.5" onClick={onRetry}>
          <RefreshCcw className="h-3.5 w-3.5" /> Retry
        </button>
      )}
    </div>
  );
}

export function CodeBlock({ children, className = '' }: { children: ReactNode; className?: string }) {
  return (
    <pre className={cx('mono overflow-x-auto whitespace-pre rounded-xl border border-white/10 bg-black/40 p-4 text-[13px] leading-relaxed text-gray-300', className)}>
      {children}
    </pre>
  );
}

export function SuccessIcon({ className = 'h-4 w-4' }: { className?: string }) {
  return <CheckCircle2 className={cx(className, 'text-emerald-400')} />;
}