import { useState } from 'react';

type Tab = 'home' | 'browse' | 'instances' | 'governance' | 'ai' | 'settings';

interface SidebarProps {
  tabs: { id: Tab; label: string; icon: string }[];
  activeTab: Tab;
  onSelectTab: (tab: Tab) => void;
  onOpenCommandPalette?: () => void;
}

export function Sidebar({ tabs, activeTab, onSelectTab, onOpenCommandPalette }: SidebarProps) {
  const [collapsed, setCollapsed] = useState(false);

  return (
    <aside
      className={`flex flex-col border-r border-border bg-card transition-all duration-200 ${
        collapsed ? 'w-16' : 'w-64'
      }`}
    >
      <div className={`p-4 border-b border-border ${collapsed ? 'text-center' : ''}`}>
        {collapsed ? (
          <h1 className="text-lg font-bold tracking-tight" aria-label="Agora">A</h1>
        ) : (
          <>
            <h1 className="text-lg font-bold tracking-tight">Agora</h1>
            <p className="text-xs text-muted-foreground mt-1">Boutique mod discovery</p>
          </>
        )}
      </div>

      <button
        onClick={() => setCollapsed(!collapsed)}
        className="absolute -right-3 top-20 z-10 h-6 w-6 rounded-full border border-border bg-card flex items-center justify-center text-xs text-muted-foreground hover:bg-accent"
        aria-label={collapsed ? 'Expand sidebar' : 'Collapse sidebar'}
      >
        {collapsed ? '→' : '←'}
      </button>

      <nav className="flex-1 p-3 space-y-1" aria-label="Main navigation">
        {tabs.map((tab) => {
          const isActive = activeTab === tab.id;
          return (
            <button
              key={tab.id}
              onClick={() => onSelectTab(tab.id)}
              aria-current={isActive ? 'page' : undefined}
              title={collapsed ? tab.label : undefined}
              className={[
                'w-full flex items-center gap-3 rounded-lg px-3 py-2.5 text-sm font-medium transition-colors',
                collapsed ? 'justify-center px-0' : '',
                isActive
                  ? 'bg-accent text-accent-foreground'
                  : 'text-foreground hover:bg-accent',
              ].join(' ')}
            >
              <span className="text-lg" aria-hidden="true">{tab.icon}</span>
              {!collapsed && tab.label}
            </button>
          );
        })}
      </nav>

      {!collapsed && (
        <div className="p-3 border-t border-border space-y-1">
          <button
            onClick={onOpenCommandPalette}
            className="w-full flex items-center gap-3 rounded-lg px-3 py-2 text-sm font-medium text-muted-foreground hover:bg-accent hover:text-foreground transition-colors"
            aria-label="Open command palette"
          >
            <kbd className="inline-flex items-center gap-1 rounded border border-border px-1.5 py-0.5 text-[10px] font-mono">
              <span>⌘</span><span>K</span>
            </kbd>
            <span>Quick actions</span>
          </button>
        </div>
      )}

      {!collapsed && (
        <div className="p-4 text-xs text-muted-foreground border-t border-border">
          v0.1.0 · Community curated
        </div>
      )}
    </aside>
  );
}
