import { useState } from 'react';

export function Settings() {
  const [modrinth, setModrinth] = useState(false);
  const [aiMcp, setAiMcp] = useState(false);

  return (
    <div className="space-y-6">
      <section>
        <h2 className="text-2xl font-bold mb-2">⚙️ Settings</h2>
        <p className="text-[rgb(var(--muted))]">
          Integration toggles, launcher path, and application preferences.
        </p>
      </section>

      <div className="rounded-xl border border-gray-200 dark:border-gray-700 surface p-4 space-y-4">
        <h3 className="font-semibold">External Services</h3>

        <label className="flex items-center justify-between">
          <span className="text-sm">Modrinth Access</span>
          <input
            type="checkbox"
            checked={modrinth}
            onChange={(e) => setModrinth(e.target.checked)}
            className="h-5 w-5 accent-brand-600"
          />
        </label>
        <p className="text-xs text-[rgb(var(--muted))]">
          Allow live Modrinth API queries and show Modrinth-sourced curated mods.
        </p>

        <label className="flex items-center justify-between pt-2 border-t border-gray-200 dark:border-gray-700">
          <span className="text-sm">AI / MCP Server</span>
          <input
            type="checkbox"
            checked={aiMcp}
            onChange={(e) => setAiMcp(e.target.checked)}
            className="h-5 w-5 accent-brand-600"
          />
        </label>
        <p className="text-xs text-[rgb(var(--muted))]">
          Enable the local MCP server for external AI tools.
        </p>
      </div>

      <p className="text-sm text-[rgb(var(--muted))]">
        TODO: Persist toggles to tauri-plugin-store / local_state.db.
      </p>
    </div>
  );
}
