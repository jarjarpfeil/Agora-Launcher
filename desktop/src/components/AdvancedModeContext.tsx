import { createContext, useContext, useState, useEffect, type ReactNode } from 'react';
import { getSetting, setSetting } from '@/lib/tauri';

interface AdvancedModeContextType {
  advancedMode: boolean;
  toggleAdvanced: () => void;
}

const AdvancedModeContext = createContext<AdvancedModeContextType>({ advancedMode: false, toggleAdvanced: () => {} });

export function AdvancedModeProvider({ children }: { children: ReactNode }) {
  const [advancedMode, setAdvancedMode] = useState(false);

  useEffect(() => {
    getSetting('advanced_mode').then(v => setAdvancedMode(v === 'true')).catch(() => {});
  }, []);

  const toggleAdvanced = () => {
    const next = !advancedMode;
    setAdvancedMode(next);
    setSetting('advanced_mode', String(next));
  };

  return (
    <AdvancedModeContext.Provider value={{ advancedMode, toggleAdvanced }}>
      {children}
    </AdvancedModeContext.Provider>
  );
}

export const useAdvancedMode = () => useContext(AdvancedModeContext);
