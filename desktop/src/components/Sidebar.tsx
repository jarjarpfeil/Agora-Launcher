import { useState } from 'react';
import type { LucideIcon } from 'lucide-react';
import { ChevronLeft, ChevronRight, Command } from 'lucide-react';
import { BrandMark } from './BrandMark';

type Tab = 'home' | 'browse' | 'instances' | 'governance' | 'ai' | 'settings';

interface SidebarProps {
  tabs: { id: Tab; label: string; icon: LucideIcon }[];
  activeTab: Tab;
  onSelectTab: (tab: Tab) => void;
  onOpenCommandPalette?: () => void;
}

export function Sidebar({ tabs, activeTab, onSelectTab, onOpenCommandPalette }: SidebarProps) {
  const [collapsed, setCollapsed] = useState(false);

  return (
    <aside className={`relative flex flex-col border-r border-border bg-card/95 shadow-[4px_0_24px_hsl(var(--midnight)/0.04)] backdrop-blur transition-all duration-200 ${collapsed ? 'w-16' : 'w-64'}`}>
      <div className={`border-b border-border ${collapsed ? 'p-3' : 'p-4'}`}>
        <BrandMark compact={collapsed} className={collapsed ? 'justify-center' : ''} />
      </div>

      <button
        onClick={() => setCollapsed(!collapsed)}
        className="absolute -right-3 top-20 z-10 flex h-6 w-6 items-center justify-center rounded-full border border-border bg-card text-muted-foreground shadow-sm hover:bg-accent"
        aria-label={collapsed ? 'Expand sidebar' : 'Collapse sidebar'}
      >
        {collapsed ? <ChevronRight className="h-3.5 w-3.5" /> : <ChevronLeft className="h-3.5 w-3.5" />}
      </button>

      <nav className="flex-1 space-y-1 p-3" aria-label="Main navigation">
        {tabs.map((tab) => {
          const isActive = activeTab === tab.id;
          const Icon = tab.icon;
          return (
            <button
              key={tab.id}
              onClick={() => onSelectTab(tab.id)}
              aria-current={isActive ? 'page' : undefined}
              title={collapsed ? tab.label : undefined}
              className={[
                'flex w-full items-center gap-3 rounded-lg px-3 py-2.5 text-sm font-medium transition-colors',
                collapsed ? 'justify-center px-0' : '',
                isActive
                  ? 'bg-primary text-primary-foreground shadow-sm'
                  : 'text-muted-foreground hover:bg-accent hover:text-accent-foreground',
              ].join(' ')}
            >
              <Icon className="h-[18px] w-[18px] shrink-0" aria-hidden="true" />
              {!collapsed && tab.label}
            </button>
          );
        })}
      </nav>

      {!collapsed && (
        <div className="space-y-1 border-t border-border p-3">
          <button
            onClick={onOpenCommandPalette}
            className="flex w-full items-center gap-3 rounded-lg px-3 py-2 text-sm font-medium text-muted-foreground transition-colors hover:bg-accent hover:text-foreground"
            aria-label="Open command palette"
          >
            <Command className="h-4 w-4" aria-hidden="true" />
            <span>Quick actions</span>
            <kbd className="ml-auto rounded border border-border bg-background px-1.5 py-0.5 font-mono text-[10px]">Ctrl K</kbd>
          </button>
        </div>
      )}

      {!collapsed && (
        <div className="border-t border-border p-4 text-xs text-muted-foreground">v0.1.0 · Community curated</div>
      )}
    </aside>
  );
}
