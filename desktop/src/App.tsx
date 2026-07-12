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
import { HealthDialog } from './components/HealthDialog';
import { CrashInvestigator } from './components/CrashInvestigator';
import { ToastContainer } from './components/Toast';
import { useDestination, type Destination, type Tab } from './lib/useDestination';
import { useProcessController } from './lib/useProcessController';

const BASE_TABS: { id: Tab; label: string; icon: string }[] = [
  { id: 'home', label: 'Home', icon: '\u{1F3E0}' },
  { id: 'browse', label: 'Browse', icon: '\u{1F50D}' },
  { id: 'instances', label: 'My Instances', icon: '\u{1F4E6}' },
  { id: 'governance', label: 'Community Governance', icon: '\u{1F5F3}\u{FE0F}' },
  { id: 'settings', label: 'Settings', icon: '\u{2699}\u{FE0F}' },
];

const AI_TAB: { id: Tab; label: string; icon: string } = {
  id: 'ai',
  label: 'AI Assistant',
  icon: '\u{1F916}',
};

/**
 * Parse a stored boolean setting strictly.
 * - `true` / `false` → as-is
 * - `"true"` / `"1"` → true
 * - `"false"` / `"0"` → false
 * - Everything else (including `null`, missing, corrupt) → fallback
 */
function parseStoredBoolean(value: unknown, fallback: boolean): boolean {
  if (typeof value === 'boolean') return value;
  if (typeof value === 'string') {
    if (value === 'true' || value === '1') return true;
    if (value === 'false' || value === '0') return false;
  }
  if (typeof value === 'number') return value === 1;
  return fallback;
}

/** Minimal branded loading shell shown while async initialization runs. */
function BrandedSplash() {
  return (
    <div className="flex h-screen w-screen items-center justify-center bg-background">
      <div className="text-center">
        <h1 className="text-3xl font-bold text-foreground">Agora</h1>
        <p className="mt-2 text-sm text-muted-foreground">Loading…</p>
        <div className="mt-4 flex justify-center">
          <div className="h-5 w-5 animate-spin rounded-full border-2 border-primary border-t-transparent" />
        </div>
      </div>
    </div>
  );
}

/** Derive the effective tab from a destination. */
function destToTab(dest: Destination): Tab {
  if (dest.type === 'tab') return dest.tab;
  if (dest.type === 'instance-detail') return 'instances';
  return 'home'; // mod-detail doesn't change the tab
}

export default function App() {
  const {
    destination,
    navigateToTab,
    navigateToModDetail,
    navigateToInstanceDetail,
  } = useDestination();

  const processController = useProcessController();

  const [onboardingComplete, setOnboardingComplete] = useState<boolean | null>(null);
  const [aiChatEnabled, setAiChatEnabled] = useState<boolean>(false);
  const [commandPaletteOpen, setCommandPaletteOpen] = useState(false);
  const [crashInvestigation, setCrashInvestigation] = useState<{
    instanceId: string;
    crashFilename: string | null;
    manualLogText: string | null;
    directLaunch: boolean;
  } | null>(null);

  // Legacy bridge: the CommandPalette still uses (tab, instanceId?) signature.
  const handleNavigate = (tab: Tab, instanceId?: string) => {
    if (instanceId) {
      navigateToInstanceDetail(instanceId);
    } else {
      navigateToTab(tab);
    }
  };

  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const value = await getSetting('onboarding_complete');
        if (!cancelled) setOnboardingComplete(parseStoredBoolean(value, false));
      } catch {
        // On transient read failure, assume completed (safe for non-Tauri dev).
        if (!cancelled) setOnboardingComplete(true);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  // Re-read the ai_chat_enabled toggle whenever the destination changes
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
  }, [destination]);

  // React to the agora-navigate custom event (used by external code).
  useEffect(() => {
    const handler = (e: Event) => {
      const detail = (e as CustomEvent).detail as string;
      if (detail === 'settings') {
        navigateToTab('settings');
      }
    };
    window.addEventListener('agora-navigate', handler);
    return () => window.removeEventListener('agora-navigate', handler);
  }, [navigateToTab]);

  // Ctrl+K / Cmd+K opens the command palette.
  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key === 'k') {
        const target = e.target instanceof HTMLElement ? e.target : null;
        const tag = target?.tagName;
        if (tag === 'INPUT' || tag === 'TEXTAREA' || target?.isContentEditable || target?.getAttribute('role') === 'textbox') {
          return;
        }
        e.preventDefault();
        setCommandPaletteOpen((prev) => !prev);
      }
    };
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, []);

  if (onboardingComplete === null) {
    return <BrandedSplash />;
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

  // Resolve the current UI state from the destination.
  const effectiveTab: Tab =
    destination.type === 'tab' && destination.tab === 'ai' && !aiChatEnabled
      ? 'home'
      : destToTab(destination);

  const showInstanceEditor = destination.type === 'instance-detail';
  const showModDetail = destination.type === 'mod-detail';

  // Render the HealthDialog at the App level so it survives page navigation.
  const {
    state: processState,
    logs: processLogs,
    startLaunch,
    approveLaunch,
    cancelLaunch,
    kill: killProcess,
  } = processController;

  return (
    <div className="flex h-screen w-screen overflow-hidden">
        <OfflineBanner />
        <Sidebar
          tabs={tabs}
          activeTab={effectiveTab}
          onSelectTab={navigateToTab}
          onOpenCommandPalette={() => setCommandPaletteOpen(true)}
        />

        {(processState.phase === 'awaiting-decision' || processState.phase === 'launching') && processState.healthReport && (
          <HealthDialog
            instanceId={processState.instanceId!}
            instanceName={processState.instanceId!}
            initialReport={processState.healthReport}
            onConfirm={approveLaunch}
            onCancel={cancelLaunch}
          />
        )}

        <main className="flex-1 overflow-y-auto p-6 bg-background">
          {showInstanceEditor ? (
            <InstanceEditor
              instanceId={destination.instanceId}
              onBack={() => navigateToTab('instances')}
              onOpenInstanceEditor={(id) => navigateToInstanceDetail(id)}
            />
          ) : showModDetail ? (
            <ModDetail
              itemId={destination.itemId}
              onBack={() => navigateToTab('browse')}
              onOpenInstanceEditor={(id) => {
                navigateToInstanceDetail(id);
              }}
            />
          ) : (
            <>
              {effectiveTab === 'home' && (
                <Home
                  onNavigateTab={navigateToTab}
                  onOpenInstance={navigateToInstanceDetail}
                  onOpenMod={navigateToModDetail}
                  onLaunch={startLaunch}
                />
              )}
              {effectiveTab === 'browse' && (
                <Browse
                  onSelectMod={(id) => navigateToModDetail(id)}
                />
              )}
              {effectiveTab === 'instances' && (
                <Instances
                  onEditInstance={(id) => navigateToInstanceDetail(id)}
                  processState={processState}
                  processLogs={processLogs}
                  onStartLaunch={startLaunch}
                  onKillProcess={killProcess}
                  onStartCrashInvestigation={setCrashInvestigation}
                />
              )}
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
        <ToastContainer />
        {crashInvestigation && (
          <CrashInvestigator
            instanceId={crashInvestigation.instanceId}
            crashFilename={crashInvestigation.crashFilename}
            manualLogText={crashInvestigation.manualLogText}
            onClose={() => setCrashInvestigation(null)}
            onLaunch={() => startLaunch(
              crashInvestigation.instanceId,
              crashInvestigation.directLaunch,
            )}
          />
        )}
    </div>
  );
}

