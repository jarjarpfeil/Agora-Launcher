import { createContext, useContext, useEffect, useState } from "react";

type Theme = "light" | "dark" | "system";

interface ThemeValues {
  theme: Theme;
  accentColor: string | null;
  setTheme: (theme: Theme) => void;
  setAccentColor: (color: string | null) => void;
}

const ThemeContext = createContext<ThemeValues | null>(null);

const STORAGE_KEY = "agora-theme";

function loadStored(): { theme: Theme; accentColor: string | null } | null {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (raw) return JSON.parse(raw);
  } catch {
    /* corrupted — ignore */
  }
  return null;
}

function storeStored(data: { theme: Theme; accentColor: string | null }) {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(data));
  } catch {
    /* quota — ignore */
  }
}

/* Tauri invoke helper — wrapped in try/catch because the Rust command
   does not exist yet. Falls back to a warm-amber default (beer branding). */
async function getWindowsAccentColor(): Promise<string | null> {
  try {
    // TODO(phase-10): wire to real Rust command `get_windows_accent_color`
    // Once the Rust side is implemented, replace this stub with:
    //   const { invoke } = await import("@tauri-apps/api/core");
    //   const result = await invoke<string>("get_windows_accent_color");
    //   return result;
    return null;
  } catch {
    // Rust command not yet available — fall back to warm amber accent
    return "hsl(35 90% 55%)";
  }
}

export function ThemeProvider({ children }: { children: React.ReactNode }) {
  const [theme, setThemeState] = useState<Theme>(() => {
    const stored = loadStored();
    return stored?.theme ?? "system";
  });

  const [accentColor, setAccentColorState] = useState<string | null>(null);
  const [mounted, setMounted] = useState(false);

  // Hydrate from localStorage on mount
  useEffect(() => {
    const stored = loadStored();
    if (stored) {
      setThemeState(stored.theme);
      if (stored.accentColor) {
        setAccentColorState(stored.accentColor);
      }
    }
  }, []);

  // Apply light/dark class to <html> based on theme + OS preference
  useEffect(() => {
    const mediaQuery = window.matchMedia("(prefers-color-scheme: dark)");

    function applyTheme() {
      const isDark =
        theme === "dark" ||
        (theme === "system" && mediaQuery.matches);
      document.documentElement.classList.toggle("dark", isDark);
    }

    applyTheme();

    const handler = () => applyTheme();
    mediaQuery.addEventListener("change", handler);
    return () => mediaQuery.removeEventListener("change", handler);
  }, [theme]);

  // Persist theme + accent changes to localStorage
  useEffect(() => {
    storeStored({ theme, accentColor });
  }, [theme, accentColor]);

  // Fetch Windows accent color once on mount
  useEffect(() => {
    let cancelled = false;
    getWindowsAccentColor().then((color) => {
      if (!cancelled) {
        setAccentColorState(color);
        setMounted(true);
      }
    });
    return () => {
      cancelled = true;
    };
  }, []);

  const setTheme = (t: Theme) => setThemeState(t);
  const setAccentColor = (c: string | null) => setAccentColorState(c);

  // Prevent flash of wrong theme
  if (!mounted) {
    return null;
  }

  return (
    <ThemeContext.Provider
      value={{ theme, accentColor, setTheme, setAccentColor }}
    >
      {children}
    </ThemeContext.Provider>
  );
}

export function useTheme(): ThemeValues {
  const ctx = useContext(ThemeContext);
  if (!ctx) throw new Error("useTheme must be used within ThemeProvider");
  return ctx;
}
