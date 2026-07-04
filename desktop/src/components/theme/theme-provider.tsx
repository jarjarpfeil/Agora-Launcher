import { createContext, useContext, useEffect, useState } from "react";
import { getWindowsAccentColor } from "@/lib/tauri";

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

/* Tauri invoke helper — wrapped in try/catch. Falls back to null (amber default). */
async function fetchWindowsAccentColor(): Promise<string | null> {
  try {
    return await getWindowsAccentColor();
  } catch {
    return null;
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

  // Fetch Windows accent color once on mount and apply as CSS variable
  useEffect(() => {
    let cancelled = false;
    fetchWindowsAccentColor().then((color) => {
      if (!cancelled) {
        setAccentColorState(color);
        setMounted(true);
      }
    });
    return () => {
      cancelled = true;
    };
  }, []);

  // Sync accent color to CSS variable --accent
  useEffect(() => {
    const root = document.documentElement;
    if (accentColor) {
      root.style.setProperty("--accent", accentColor);
    } else {
      root.style.removeProperty("--accent");
    }
  }, [accentColor]);

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
