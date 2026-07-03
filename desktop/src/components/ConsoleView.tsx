import { useState, useEffect, useRef, useCallback } from 'react';
import { listen } from '@tauri-apps/api/event';
import { cn } from '@/lib/utils';
import { Button } from '@/components/ui/button';

interface GameLogEvent { line: string; stream: 'stdout' | 'stderr'; }
interface Props { instanceId: string; className?: string; }
const MAX_LINES = 10000;

export function ConsoleView({ instanceId, className }: Props) {
  const [logs, setLogs] = useState<{ line: string; stream: string; level: string }[]>([]);
  const [autoScroll, setAutoScroll] = useState(true);
  const [filter, setFilter] = useState<Set<string>>(new Set(['INFO','WARN','ERROR','DEBUG']));
  const endRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const unlisten = listen<GameLogEvent>('game-log', (e) => {
      const level = e.payload.line.includes('[ERROR]') ? 'ERROR'
        : e.payload.line.includes('[WARN]') ? 'WARN'
        : e.payload.line.includes('[DEBUG]') ? 'DEBUG'
        : 'INFO';
      setLogs(prev => {
        const next = [...prev, { line: e.payload.line, stream: e.payload.stream, level }];
        return next.length > MAX_LINES ? next.slice(-MAX_LINES) : next;
      });
    });
    return () => { unlisten.then(fn => fn()); };
  }, [instanceId]);

  useEffect(() => {
    if (autoScroll) endRef.current?.scrollIntoView({ behavior: 'smooth' });
  }, [logs, autoScroll]);

  const onScroll = useCallback((e: React.UIEvent<HTMLDivElement>) => {
    const el = e.currentTarget;
    const atBottom = el.scrollHeight - el.scrollTop - el.clientHeight < 30;
    setAutoScroll(atBottom);
  }, []);

  const clear = () => setLogs([]);

  const copyVisible = useCallback(() => {
    const visible = filtered().map(l => l.line).join('\n');
    navigator.clipboard.writeText(visible);
  }, [logs, filter]);

  const filtered = () => logs.filter(l => filter.has(l.level));

  const toggle = (level: string) => {
    setFilter(prev => {
      const next = new Set(prev);
      next.has(level) ? next.delete(level) : next.add(level);
      return next;
    });
  };

  const levelColor: Record<string, string> = {
    ERROR: 'text-destructive',
    WARN: 'text-amber-500',
    DEBUG: 'text-muted-foreground',
    INFO: 'text-foreground',
  };

  const visible = filtered();

  return (
    <div className={cn('rounded-lg border border-border bg-background', className)}>
      <div className="flex items-center gap-2 border-b border-border px-3 py-1.5">
        {['INFO','WARN','ERROR','DEBUG'].map(l => (
          <button key={l} onClick={() => toggle(l)}
            className={cn('rounded px-1.5 py-0.5 text-xs font-medium',
              filter.has(l) ? 'bg-primary/20 text-primary' : 'text-muted-foreground opacity-50')}>
            {l}
          </button>
        ))}
        <div className="flex-1" />
        <span className="text-xs text-muted-foreground">{visible.length} lines</span>
        <Button variant="ghost" size="sm" onClick={copyVisible} className="h-6 text-xs">Copy</Button>
        <Button variant="ghost" size="sm" onClick={clear} className="h-6 text-xs">Clear</Button>
      </div>
      <div onScroll={onScroll} className="overflow-auto max-h-96 p-2 font-mono text-xs leading-relaxed">
        {visible.map((l, i) => (
          <div key={i} className={cn(levelColor[l.level], l.stream === 'stderr' ? 'border-l-2 border-destructive pl-1' : '')}>
            {l.line}
          </div>
        ))}
        <div ref={endRef} />
      </div>
    </div>
  );
}
