import { useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import {
  Dialog,
  DialogContent,
  DialogTitle,
  DialogDescription,
} from '@/components/ui/dialog';
import { Input } from '@/components/ui/input';
import { cn } from '@/lib/utils';
import type { InstanceRow } from '@/lib/tauri';

interface CommandPaletteProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onNavigate: (tab: 'home' | 'browse' | 'instances' | 'governance' | 'settings' | 'ai', clearSelection?: boolean) => void;
}

const SETTINGS_ITEMS: { label: string; tab: 'home' | 'browse' | 'instances' | 'governance' | 'settings'; icon: string }[] = [
  { label: 'Home', tab: 'home', icon: '🏠' },
  { label: 'Browse', tab: 'browse', icon: '🔍' },
  { label: 'My Instances', tab: 'instances', icon: '📦' },
  { label: 'Community Governance', tab: 'governance', icon: '🗳️' },
  { label: 'Settings', tab: 'settings', icon: '⚙️' },
];

export function CommandPalette({ open, onOpenChange, onNavigate }: CommandPaletteProps) {
  const [query, setQuery] = useState('');
  const [selectedIndex, setSelectedIndex] = useState(0);
  const [instances, setInstances] = useState<InstanceRow[]>([]);
  const [instancesLoaded, setInstancesLoaded] = useState(false);
  const searchRef = useRef<HTMLInputElement>(null);
  const keydownRef = useRef<((e: KeyboardEvent) => void) | null>(null);

  // Fetch instances once per open cycle
  useEffect(() => {
    if (open && !instancesLoaded) {
      setInstancesLoaded(false);
      invoke<InstanceRow[]>('list_instances')
        .then((data) => {
          setInstances(data);
          setInstancesLoaded(true);
        })
        .catch(() => {
          setInstances([]);
          setInstancesLoaded(true);
        });
    }
    if (!open) {
      setQuery('');
      setSelectedIndex(0);
      setInstancesLoaded(false);
    }
  }, [open]);

  // Auto-focus search input on open
  useEffect(() => {
    if (open) {
      requestAnimationFrame(() => searchRef.current?.focus());
    }
  }, [open]);

  // Keyboard navigation: arrow keys, Enter, Escape
  useEffect(() => {
    if (!open) return;

    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'ArrowDown') {
        e.preventDefault();
        setSelectedIndex((prev) => (prev + 1) % totalItems);
      } else if (e.key === 'ArrowUp') {
        e.preventDefault();
        setSelectedIndex((prev) => (prev - 1 + totalItems) % totalItems);
      } else if (e.key === 'Enter') {
        e.preventDefault();
        if (totalItems > 0 && selectedIndex >= 0 && selectedIndex < totalItems) {
          activateItem(selectedIndex);
        }
      }
    };

    window.addEventListener('keydown', handleKeyDown);
    keydownRef.current = handleKeyDown;
    return () => {
      window.removeEventListener('keydown', handleKeyDown);
      keydownRef.current = null;
    };
  }, [open, query, selectedIndex, instancesLoaded]);

  // Build flattened results
  const filteredInstances = instances.filter((inst) =>
    query ? inst.name.toLowerCase().includes(query.toLowerCase()) : true
  );

  const filteredSettings = SETTINGS_ITEMS.filter((item) =>
    query ? item.label.toLowerCase().includes(query.toLowerCase()) : true
  );

  const results = [
    ...(filteredInstances.length > 0
      ? [{ __section: 'Instances' as const }, ...filteredInstances.map((i) => ({ __type: 'instance' as const, ...i }))]
      : []),
    ...(filteredSettings.length > 0
      ? [{ __section: 'Settings' as const }, ...filteredSettings.map((s) => ({ __type: 'setting' as const, ...s }))]
      : []),
  ];

  const totalItems = results.length;

  const activateItem = (index: number) => {
    const item = results[index];
    if (!item) return;

    if ('instance_id' in item && item.__type === 'instance') {
      onOpenChange(false);
      onNavigate('instances');
    } else if ('tab' in item && item.__type === 'setting') {
      onOpenChange(false);
      onNavigate(item.tab);
    }
    // section headers do nothing
  };

  const isItemSelected = (index: number) => index === selectedIndex;

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-2xl p-0 gap-0 overflow-hidden">
        <DialogTitle className="sr-only">Command Palette</DialogTitle>
        <DialogDescription className="sr-only">
          Search and navigate across instances, settings, and the catalog.
        </DialogDescription>

        <div className="flex items-center gap-3 px-4 border-b border-gray-200 dark:border-gray-700">
          <span className="text-[rgb(var(--muted))] text-lg" aria-hidden="true">⌕</span>
          <Input
            ref={searchRef}
            value={query}
            onChange={(e) => {
              setQuery(e.target.value);
              setSelectedIndex(0);
            }}
            placeholder="Type a command or search…"
            className="border-0 shadow-none focus-visible:ring-0 focus-visible:ring-offset-0 h-12 text-base bg-transparent placeholder:text-[rgb(var(--muted))]"
          />
          <kbd className="ml-auto text-xs text-[rgb(var(--muted))] border border-gray-300 dark:border-gray-600 rounded px-1.5 py-0.5">
            ESC
          </kbd>
        </div>

        <div className="max-h-[60vh] overflow-y-auto p-2">
          {results.length === 0 ? (
            <div className="text-center py-8 text-[rgb(var(--muted))] text-sm">
              No results found.
            </div>
          ) : (
            results.map((item, index) => {
              // Section header
              if ('__section' in item) {
                return (
                  <div
                    key={item.__section}
                    className="px-3 py-1.5 text-xs font-semibold uppercase tracking-wider text-[rgb(var(--muted))]"
                  >
                    {item.__section}
                  </div>
                );
              }

              // Instance row
              if (item.__type === 'instance') {
                return (
                  <button
                    key={item.instance_id}
                    onClick={() => activateItem(index)}
                    className={cn(
                      'w-full flex items-center gap-3 px-3 py-2.5 rounded-lg text-sm transition-colors text-left',
                      isItemSelected(index)
                        ? 'bg-brand-100 text-brand-900 dark:bg-brand-900 dark:text-brand-100'
                        : 'text-[rgb(var(--text))] hover:bg-gray-100 dark:hover:bg-gray-800'
                    )}
                  >
                    <span className="text-lg" aria-hidden="true">📦</span>
                    <div className="flex-1 min-w-0">
                      <div className="font-medium truncate">{item.name}</div>
                      <div className="text-xs text-[rgb(var(--muted))] truncate">
                        {item.loader} {item.loader_version} · MC {item.minecraft_version}
                      </div>
                    </div>
                  </button>
                );
              }

              // Settings row
              if (item.__type === 'setting') {
                return (
                  <button
                    key={item.tab}
                    onClick={() => activateItem(index)}
                    className={cn(
                      'w-full flex items-center gap-3 px-3 py-2.5 rounded-lg text-sm transition-colors text-left',
                      isItemSelected(index)
                        ? 'bg-brand-100 text-brand-900 dark:bg-brand-900 dark:text-brand-100'
                        : 'text-[rgb(var(--text))] hover:bg-gray-100 dark:hover:bg-gray-800'
                    )}
                  >
                    <span className="text-lg" aria-hidden="true">{item.icon}</span>
                    <span className="font-medium">{item.label}</span>
                  </button>
                );
              }

              return null;
            })
          )}
        </div>
      </DialogContent>
    </Dialog>
  );
}
