import { useState, useEffect } from 'react';
import { listAuditLog, formatError, AuditLogEntry } from '../lib/tauri';

export function Governance() {
  const [entries, setEntries] = useState<AuditLogEntry[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    listAuditLog(200)
      .then((data) => {
        if (!cancelled) {
          setEntries(data);
          setLoading(false);
        }
      })
      .catch((e) => {
        if (!cancelled) {
          setError(formatError(e));
          setLoading(false);
        }
      });
    return () => {
      cancelled = true;
    };
  }, []);

  return (
    <div className="space-y-6">
      <section>
        <h2 className="text-2xl font-bold mb-2">Community Governance</h2>
        <p className="text-[rgb(var(--muted))]">
          Active triage polls, recent resolutions, and the transparency log.
        </p>
      </section>

      {/* Polls placeholder */}
      <div className="rounded-xl p-6 border border-dashed border-gray-300 dark:border-gray-600 text-center">
        <p className="text-[rgb(var(--muted))]">No active polls or resolutions.</p>
        <p className="text-sm text-[rgb(var(--muted))] mt-2">
          TODO: Query under_review items and live GitHub Discussions poll data.
        </p>
      </div>

      {/* Transparency Log */}
      <section className="rounded-xl border border-gray-200 dark:border-gray-700 p-6">
        <h3 className="text-lg font-semibold mb-4">Transparency Log</h3>

        {error && (
          <div className="mb-4 p-4 rounded-lg bg-red-50 dark:bg-red-900/20 border border-red-200 dark:border-red-800 text-red-700 dark:text-red-300">
            {error}
          </div>
        )}

        {loading && (
          <p className="text-[rgb(var(--muted))]">Loading transparency log…</p>
        )}

        {!loading && !error && entries.length === 0 && (
          <p className="text-[rgb(var(--muted))]">No governance actions recorded yet.</p>
        )}

        {!loading && !error && entries.length > 0 && (
          <div className="max-h-96 overflow-y-auto space-y-3 pr-2">
            {entries.map((entry) => (
              <div
                key={entry.id}
                className="p-3 rounded-lg bg-gray-50 dark:bg-[rgb(var(--surface))]/50 border border-gray-100 dark:border-gray-700"
              >
                <div className="flex items-center gap-2 mb-1">
                  <time
                    dateTime={entry.timestamp}
                    className="text-sm text-[rgb(var(--muted))] font-mono"
                  >
                    {entry.timestamp}
                  </time>
                  <span className="text-xs px-2 py-0.5 rounded-full bg-gray-200 dark:bg-gray-700 text-[rgb(var(--muted))] capitalize">
                    {entry.action}
                  </span>
                </div>
                {entry.details && (
                  <p className="text-sm text-[rgb(var(--muted))]">{entry.details}</p>
                )}
              </div>
            ))}
          </div>
        )}
      </section>
    </div>
  );
}
