import { useEffect, useState } from 'react';
import { formatError, getRegistryStatus, checkRegistryUpdate, type RegistryStatus } from '../lib/tauri';

export function Home() {
  const [status, setStatus] = useState<RegistryStatus | null>(null);
  const [checking, setChecking] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const refresh = async () => {
    try {
      setStatus(await getRegistryStatus());
    } catch (e) {
      setError(formatError(e));
    }
  };

  useEffect(() => {
    refresh();
  }, []);

  const checkForUpdates = async () => {
    setChecking(true);
    setError(null);
    try {
      const result = await checkRegistryUpdate(true);
      setStatus(result);
    } catch (e) {
      setError(formatError(e));
    } finally {
      setChecking(false);
    }
  };

  return (
    <div className="space-y-6">
      <section>
        <h2 className="text-2xl font-bold mb-2">Home</h2>
        <p className="text-[rgb(var(--muted))]">
          Featured & trending curated packs and mods.
        </p>
      </section>

      {/* Registry status card */}
      <div className="rounded-xl border border-gray-200 dark:border-gray-700 surface p-4">
        <div className="flex items-center justify-between gap-4">
          <div>
            <h3 className="font-semibold text-sm">Registry Status</h3>
            <p className="text-xs text-[rgb(var(--muted))] mt-1">
              {status?.message ?? 'Loading…'}
            </p>
            {status?.cached_tag && (
              <p className="text-xs text-[rgb(var(--muted))]">
                Cached: {status.cached_tag}
                {status.cached_schema_version != null && ` · schema v${status.cached_schema_version}`}
              </p>
            )}
            {status?.latest_tag && status.latest_tag !== status.cached_tag && (
              <p className="text-xs text-amber-600 dark:text-amber-400">
                Latest: {status.latest_tag}
              </p>
            )}
          </div>
          <button
            onClick={checkForUpdates}
            disabled={checking || !status?.has_cached_db}
            className="rounded-lg bg-brand-600 px-3 py-1.5 text-xs font-medium text-white hover:bg-brand-700 disabled:opacity-50 whitespace-nowrap"
          >
            {checking
              ? 'Checking…'
              : !status?.has_cached_db
                ? 'Download Registry'
                : 'Check for Updates'}
          </button>
        </div>
        {status?.update_available && (
          <div className="mt-2 rounded-md bg-amber-50 dark:bg-amber-900/20 px-3 py-1.5 text-xs text-amber-700 dark:text-amber-300">
            An update is available. Click "Check for Updates" to download.
          </div>
        )}
        {error && (
          <p className="mt-2 text-xs text-red-600 dark:text-red-300">{error}</p>
        )}
      </div>

      <div className="rounded-xl p-6 border border-dashed border-gray-300 dark:border-gray-600 text-center">
        <p className="text-[rgb(var(--muted))]">No featured items yet.</p>
        <p className="text-sm text-[rgb(var(--muted))] mt-2">
          {status?.has_cached_db
            ? 'Browse the catalog to discover curated mods and packs.'
            : 'Download the registry to browse curated content.'}
        </p>
      </div>
    </div>
  );
}
