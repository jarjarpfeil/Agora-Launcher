import { useCallback, useEffect, useRef, useState } from 'react';

export type Tab = 'home' | 'browse' | 'instances' | 'governance' | 'ai' | 'settings';

/**
 * A single typed application destination. Replaces the previous pattern of
 * three independent state variables (activeTab, selectedModId, editingInstanceId).
 *
 * Destinations:
 * - `tab` — one of the sidebar tabs (home, browse, instances, governance, ai, settings).
 * - `mod-detail` — browsing a specific curated item.
 * - `instance-detail` — editing a specific instance.
 */
export type Destination =
  | { type: 'tab'; tab: Tab }
  | { type: 'mod-detail'; itemId: string }
  | { type: 'instance-detail'; instanceId: string };

export interface UseDestinationReturn {
  destination: Destination;
  canGoBack: boolean;
  navigate: (dest: Destination) => void;
  goBack: () => void;
  navigateToTab: (tab: Tab) => void;
  navigateToModDetail: (itemId: string) => void;
  navigateToInstanceDetail: (instanceId: string) => void;
}

const MAX_HISTORY = 50;

export function useDestination(): UseDestinationReturn {
  const historyRef = useRef<Destination[]>([{ type: 'tab', tab: 'home' }]);
  const [destination, setDestination] = useState<Destination>(historyRef.current[0]);
  const [canGoBack, setCanGoBack] = useState(false);

  const push = useCallback((dest: Destination) => {
    historyRef.current.push(dest);
    if (historyRef.current.length > MAX_HISTORY) {
      historyRef.current = historyRef.current.slice(-MAX_HISTORY);
    }
    setCanGoBack(historyRef.current.length > 1);
    setDestination(dest);
    window.history.pushState(dest, '');
  }, []);

  // Handle browser back/forward via popstate.
  useEffect(() => {
    // Seed the initial history entry so popstate restores the correct state.
    window.history.replaceState(historyRef.current[0], '');

    const handlePopState = (e: PopStateEvent) => {
      if (e.state) {
        const restored = e.state as Destination;
        setDestination(restored);
        // Sync the history ref so follow-up forward/back is consistent.
        historyRef.current.push(restored);
        if (historyRef.current.length > MAX_HISTORY) {
          historyRef.current = historyRef.current.slice(-MAX_HISTORY);
        }
      } else {
        // No state at all — fall back to home.
        setDestination({ type: 'tab', tab: 'home' });
      }
    };
    window.addEventListener('popstate', handlePopState);
    return () => window.removeEventListener('popstate', handlePopState);
  }, []);

  const navigate = useCallback((dest: Destination) => push(dest), [push]);
  const goBack = useCallback(() => window.history.back(), []);
  const navigateToTab = useCallback((tab: Tab) => push({ type: 'tab', tab }), [push]);
  const navigateToModDetail = useCallback((itemId: string) => push({ type: 'mod-detail', itemId }), [push]);
  const navigateToInstanceDetail = useCallback(
    (instanceId: string) => push({ type: 'instance-detail', instanceId }),
    [push],
  );

  return {
    destination,
    canGoBack,
    navigate,
    goBack,
    navigateToTab,
    navigateToModDetail,
    navigateToInstanceDetail,
  };
}
