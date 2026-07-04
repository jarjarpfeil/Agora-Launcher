import { useEffect, useState } from 'react';
import { Sidebar } from './components/Sidebar';
import { CommandPalette } from './components/command-palette';
import { Home } from './pages/Home';
import { Browse } from './pages/Browse';
import { Instances } from './pages/Instances';
import { Governance } from './pages/Governance';
import { Settings } from './pages/Settings';
import AiChatPage from './pages/AiChatPage';
import { Onboarding } from './pages/Onboarding';
import { ModDetail } from './pages/ModDetail';
import { InstanceEditor } from './pages/InstanceEditor';
import { getSetting } from './lib/tauri';
import { OfflineBanner } from './components/offline-banner';

type Tab = 'home' | 'browse' | 'instances' | 'governance' | 'ai' | 'settings';

const BASE_TABS: { id: Tab; label: string; icon: string }[] = [
  { id: 'home', label: 'Home', icon: '🏠' },
  { id: 'browse', label: 'Browse', icon: '🔍' },
  { id: 'instances', label: 'My Instances', icon: '📦' },
  { id: 'governance', label: 'Community Governance', icon: '🗳️' },
  { id: 'settings', label: 'Settings', icon: '⚙️' },
];

const AI_TAB: { id: Tab; label: string; icon: string } = {
  id: 'ai',
  label: 'AI Assistant',
  icon: '🤖',
};

export default function App() {
  const [activeTab, setActiveTab] = useState<Tab>('home');
  const [selectedModId, setSelectedModId] = useState<string | null>(null);
  const [editingInstanceId, setEditingInstanceId] = useState<string | null>(null);
  const [onboardingComplete, setOnboardingComplete] = useState<boolean | null>(null);
  const [aiChatEnabled, setAiChatEnabled] = useState<boolean>(false);
  const [commandPaletteOpen, setCommandPaletteOpen] = useState(false);
  // Telemetry upload prompt disabled — no aggregation endpoint exists yet ($0/month footprint). Local crash learning runs regardless. The crash_telemetry_opt_in setting is preserved for future shared-data use.
  // const [showTelemetryPrompt, setShowTelemetryPrompt] = useState(false);

  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const value = await getSetting('onboarding_complete');
        if (!cancelled) setOnboardingComplete(Boolean(value));
      } catch {
        if (!cancelled) setOnboardingComplete(true);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  // Re-read the ai_chat_enabled toggle whenever returning to a top-level tab
  // so the sidebar reflects the current setting without an app restart.
  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const ai = await getSetting('ai_chat_enabled');
        if (!cancelled) setAiChatEnabled(ai === true || ai === 'true');
      } catch {
        if (!cancelled) setAiChatEnabled(false);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [activeTab, onboardingComplete]);

  // One-time telemetry opt-in prompt: show only when the setting has never been set.
  // useEffect(() => {
  //   let cancelled = false;
  //   (async () => {
  //     try {
  //       const v = await getSetting('crash_telemetry_opt_in');
  //       if (!cancelled) setShowTelemetryPrompt(v === null || v === undefined);
  //     } catch {
  //       if (!cancelled) setShowTelemetryPrompt(true);
  //     }
  //   })();
  //   return () => {
  //     cancelled = true;
  //   };
  // }, []);

  // const handleTelemetryChoice = async (allow: boolean) => {
  //   try {
  //     await setSetting('crash_telemetry_opt_in', allow);
  //   } finally {
  //     setShowTelemetryPrompt(false);
  //   }
  // };

  // Global Ctrl+K / Cmd+K shortcut to open command palette
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.key === 'k') {
        e.preventDefault();
        setCommandPaletteOpen((prev) => !prev);
      }
    };
    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, []);

  const handleNavigate = (tab: Tab, clearSelection = true) => {
    if (clearSelection) {
      setSelectedModId(null);
      setEditingInstanceId(null);
    }
    setActiveTab(tab);
  };

  // Listen for cross-component navigation requests (e.g. offline banner → Settings)
  useEffect(() => {
    const handler = (e: Event) => {
      const detail = (e as CustomEvent).detail as string;
      if (detail === 'settings') {
        handleNavigate('settings');
      }
    };
    window.addEventListener('agora-navigate', handler);
    return () => window.removeEventListener('agora-navigate', handler);
  }, [handleNavigate]);

  if (onboardingComplete === null) {
    return null;
  }

  if (!onboardingComplete) {
    return (
      <div className="h-screen w-screen overflow-hidden bg-card">
        <Onboarding onComplete={() => setOnboardingComplete(true)} />
      </div>
    );
  }

  // The AI Assistant tab appears between Governance and Settings when enabled.
  const tabs = [
    BASE_TABS[0],
    BASE_TABS[1],
    BASE_TABS[2],
    BASE_TABS[3],
    ...(aiChatEnabled ? [AI_TAB] : []),
    BASE_TABS[4],
  ];

  // If the user disables AI while on that tab, bounce back home.
  const effectiveTab: Tab =
    activeTab === 'ai' && !aiChatEnabled
      ? 'home' : activeTab;

  return (
    <div className="flex h-screen w-screen overflow-hidden">
        <OfflineBanner />
        {/* Telemetry upload prompt disabled — no aggregation endpoint exists yet ($0/month footprint). Local crash learning runs regardless. The crash_telemetry_opt_in setting is preserved for future shared-data use. */}
        {/* {showTelemetryPrompt && (
          <div className="fixed bottom-4 right-4 z-50 max-w-sm rounded-xl border border-gray-200 dark:border-gray-700 surface p-4 shadow-lg">
            <p className="text-sm font-medium mb-2">Help improve Agora</p>
            <p className="text-xs text-[rgb(var(--muted))] mb-3">
              Allow anonymous crash telemetry to be collected for mod-incompatibility research?
            </p>
            <div className="flex gap-2">
              <button
                onClick={() => handleTelemetryChoice(true)}
                className="rounded-lg bg-brand-600 px-3 py-1.5 text-xs font-medium text-white hover:bg-brand-700"
              >
                Allow
              </button>
              <button
                onClick={() => handleTelemetryChoice(false)}
                className="rounded-lg border border-gray-300 dark:border-gray-600 px-3 py-1.5 text-xs font-medium text-gray-700 dark:text-gray-300 hover:bg-gray-100 dark:hover:bg-gray-800"
              >
                Not now
              </button>
            </div>
          </div>
        )} */}
        <Sidebar tabs={tabs} activeTab={effectiveTab} onSelectTab={(t) => { setSelectedModId(null); setEditingInstanceId(null); setActiveTab(t); }} />
        <main className="flex-1 overflow-y-auto p-6 bg-background">
          {editingInstanceId !== null ? (
            <InstanceEditor instanceId={editingInstanceId} onBack={() => setEditingInstanceId(null)} onOpenInstanceEditor={(id) => setEditingInstanceId(id)} />
          ) : selectedModId !== null ? (
            <ModDetail itemId={selectedModId} onBack={() => setSelectedModId(null)} onOpenInstanceEditor={(id) => { setSelectedModId(null); setEditingInstanceId(id); }} />
          ) : (
            <>
              {effectiveTab === 'home' && <Home />}
              {effectiveTab === 'browse' && (
                <Browse
                  onSelectMod={(id) => setSelectedModId(id)}
                />
              )}
              {effectiveTab === 'instances' && <Instances onEditInstance={(id) => setEditingInstanceId(id)} />}
              {effectiveTab === 'governance' && <Governance />}
              {effectiveTab === 'ai' && aiChatEnabled && <AiChatPage />}
              {effectiveTab === 'settings' && <Settings />}
            </>
          )}
        </main>

        <CommandPalette
          open={commandPaletteOpen}
          onOpenChange={setCommandPaletteOpen}
          onNavigate={handleNavigate}
        />
    </div>
  );
}
