import { useEffect, useState } from 'react';
import {
  getRegistryItem,
  listInstances,
  listModVersions,
  installModVersion,
  type RegistryItem,
  type InstanceRow,
  type ModVersionCandidate,
} from '../lib/tauri';

type CompatibleVersionEntry = Record<string, unknown> | string;

function parseCompatibleVersions(json: string | null): CompatibleVersionEntry[] {
  if (!json) return [];
  try {
    const parsed = JSON.parse(json);
    if (Array.isArray(parsed)) {
      return parsed.filter((entry): entry is CompatibleVersionEntry =>
        typeof entry === 'object' && entry !== null ? true : typeof entry === 'string',
      );
    }
    if (parsed && typeof parsed === 'object') {
      return [parsed as CompatibleVersionEntry];
    }
    return [];
  } catch {
    return [];
  }
}

function renderVersionEntry(entry: CompatibleVersionEntry): string {
  if (typeof entry === 'string') return entry;
  const fields = ['mc_version', 'minecraft_version', 'loader', 'loader_version', 'version', 'game_version'];
  const parts: string[] = [];
  for (const field of fields) {
    const value = (entry as Record<string, unknown>)[field];
    if (value != null && value !== '') parts.push(`${field}: ${String(value)}`);
  }
  if (parts.length > 0) return parts.join(' · ');
  return JSON.stringify(entry);
}

type CuratorNotesRegistryItem = RegistryItem & { curator_notes?: string | null };

export function ModDetail({ itemId, onBack }: { itemId: string; onBack: () => void }) {
  const [item, setItem] = useState<RegistryItem | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  // Install flow state
  const [showInstallFlow, setShowInstallFlow] = useState(false);
  const [instances, setInstances] = useState<InstanceRow[]>([]);
  const [selectedInstanceId, setSelectedInstanceId] = useState<string | null>(null);
  const [candidates, setCandidates] = useState<ModVersionCandidate[]>([]);
  const [selectedCandidate, setSelectedCandidate] = useState<ModVersionCandidate | null>(null);
  const [phase, setPhase] = useState<'idle' | 'loadingVersions' | 'pickingVersion' | 'installing' | 'done' | 'error'>('idle');
  const [installMsg, setInstallMsg] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        if (!cancelled) setLoading(true);
        const result = await getRegistryItem(itemId);
        if (!cancelled) {
          setItem(result);
          if (!result) setError('Mod not found in the registry.');
        }
      } catch (e) {
        if (!cancelled) setError(String(e));
      } finally {
        if (!cancelled) setLoading(false);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [itemId]);

  if (loading) {
    return (
      <div className="space-y-6">
        <BackButton onBack={onBack} />
        <div className="rounded-xl p-6 border border-dashed border-gray-300 dark:border-gray-600 text-center text-[rgb(var(--muted))]">
          Loading mod…
        </div>
      </div>
    );
  }

  if (error || !item) {
    return (
      <div className="space-y-6">
        <BackButton onBack={onBack} />
        <div className="rounded-lg border border-red-300 bg-red-50 p-3 text-sm text-red-700 dark:border-red-700 dark:bg-red-900/30 dark:text-red-200">
          {error ?? 'Mod not found.'}
        </div>
      </div>
    );
  }

  const curatorNotes = (item as CuratorNotesRegistryItem).curator_notes ?? null;
  const compatibleVersions = parseCompatibleVersions(item.compatible_versions_json);
  const showIcon = item.icon_url != null && item.icon_url.startsWith('https://');
  const velocityLabel =
    item.velocity > 0 ? `▲ ${item.velocity.toFixed(1)}` : item.velocity < 0 ? `▼ ${item.velocity.toFixed(1)}` : '0.0';

  const handleInstall = async () => {
    setShowInstallFlow(true);
    setPhase('idle');
    setInstallMsg(null);
    setSelectedInstanceId(null);
    setCandidates([]);
    setSelectedCandidate(null);
    try {
      const all = await listInstances();
      setInstances(all);
    } catch (e) {
      setPhase('error');
      setInstallMsg(String(e));
    }
  };

  const handlePickVersion = async () => {
    if (!selectedInstanceId) return;
    setPhase('loadingVersions');
    setCandidates([]);
    setSelectedCandidate(null);
    setInstallMsg(null);
    try {
      const vers = await listModVersions(selectedInstanceId, itemId);
      setCandidates(vers);
      setPhase(vers.length === 0 ? 'pickingVersion' : 'pickingVersion');
    } catch (e) {
      setPhase('error');
      setInstallMsg(String(e));
    }
  };

  const handleConfirmInstall = async () => {
    if (!selectedInstanceId || !selectedCandidate) return;
    setPhase('installing');
    setInstallMsg(null);
    try {
      await installModVersion(selectedInstanceId, itemId, selectedCandidate);
      setPhase('done');
      setInstallMsg(`Installed ${selectedCandidate.filename} to ${instances.find((i) => i.instance_id === selectedInstanceId)?.name ?? selectedInstanceId}.`);
    } catch (e) {
      setPhase('error');
      setInstallMsg(String(e));
    }
  };

  const handleCloseInstallFlow = () => {
    setShowInstallFlow(false);
    setPhase('idle');
    setInstallMsg(null);
    setSelectedInstanceId(null);
    setCandidates([]);
    setSelectedCandidate(null);
  };

  return (
    <div className="space-y-6">
      <BackButton onBack={onBack} />

      {item.is_immune && (
        <div
          className="rounded-lg border px-4 py-3 text-sm"
          style={{
            backgroundColor: 'rgba(70, 130, 180, 0.12)',
            borderColor: 'rgba(70, 130, 180, 0.6)',
            color: 'rgb(70, 130, 180)',
          }}
        >
          <div className="flex items-center gap-2 font-semibold">
            <span aria-hidden>🛡️</span>
            <span>Immunity Shield Active</span>
          </div>
          {item.immunity_reason && (
            <p className="mt-1 text-xs opacity-90">{item.immunity_reason}</p>
          )}
        </div>
      )}

      <section className="rounded-xl border border-gray-200 dark:border-gray-700 surface p-6">
        <div className="flex items-start gap-4">
          {showIcon && (
            <img
              src={item.icon_url as string}
              alt={item.name}
              className="h-16 w-16 rounded-lg border object-contain dark:border-gray-600"
            />
          )}
          <div className="flex-1 min-w-0">
            <div className="flex items-center gap-2 flex-wrap">
              <h2 className="text-2xl font-bold break-words">{item.name}</h2>
              <span className="rounded-full bg-brand-600 px-2 py-0.5 text-xs font-medium uppercase tracking-wide text-white">
                {item.content_type}
              </span>
              <span className="rounded-full border border-gray-300 dark:border-gray-600 px-2 py-0.5 text-xs text-[rgb(var(--muted))]">
                {item.download_strategy}
              </span>
              {item.status && item.status !== 'active' && (
                <span className="rounded-full border border-gray-300 dark:border-gray-600 px-2 py-0.5 text-xs text-[rgb(var(--muted))]">
                  {item.status}
                </span>
              )}
            </div>
            <p className="text-xs text-[rgb(var(--muted))] mt-1 break-all">
              {item.source_identifier}
            </p>
            <p className="text-xs text-[rgb(var(--muted))] mt-2">
              ↑ {item.upvotes} · ↓ {item.downvotes} · net {item.net_score} · velocity {velocityLabel}
            </p>
            {item.date_added && (
              <p className="text-xs text-[rgb(var(--muted))] mt-1">
                Added {item.date_added}
              </p>
            )}
          </div>
        </div>

        <div className="mt-5 flex flex-wrap gap-2">
          <button
            onClick={handleInstall}
            className="rounded-lg bg-brand-600 px-4 py-2 text-sm font-medium text-white hover:bg-brand-700"
          >
            Install to Instance
          </button>
        </div>
        {showInstallFlow && (
          <section className="mt-4 rounded-xl border border-gray-200 dark:border-gray-700 surface p-4 space-y-4">
            <div className="flex items-center justify-between">
              <h3 className="font-semibold text-sm">Install to Instance</h3>
              <button
                onClick={handleCloseInstallFlow}
                className="text-xs text-[rgb(var(--muted))] hover:text-[rgb(var(--foreground))]"
              >
                Close
              </button>
            </div>

            {phase === 'error' && installMsg && (
              <p className="text-sm text-red-600 dark:text-red-300">{installMsg}</p>
            )}

            {/* Step 1: Instance picker */}
            {!selectedInstanceId ? (
              instances.length === 0 && phase !== 'idle' ? (
                <div>
                  <p className="text-sm text-[rgb(var(--muted))]">
                    You need an instance first. Create one in the Instances tab.
                  </p>
                </div>
              ) : instances.length === 0 ? (
                <div className="text-center py-2">
                  <p className="text-sm text-[rgb(var(--muted))]">Loading instances…</p>
                </div>
              ) : (
                <div>
                  <label className="block text-xs font-medium mb-1">Select instance</label>
                  <select
                    value={selectedInstanceId ?? ''}
                    onChange={(e) => setSelectedInstanceId(e.target.value)}
                    className="w-full rounded-lg border border-gray-300 dark:border-gray-600 bg-transparent px-3 py-2 text-sm"
                  >
                    <option value="">Choose an instance…</option>
                    {instances.map((inst) => (
                      <option key={inst.instance_id} value={inst.instance_id}>
                        {inst.name} ({inst.loader} {inst.loader_version} · MC {inst.minecraft_version})
                      </option>
                    ))}
                  </select>
                  <button
                    onClick={handlePickVersion}
                    disabled={!selectedInstanceId}
                    className="mt-3 rounded-lg bg-brand-600 px-4 py-2 text-sm font-medium text-white hover:bg-brand-700 disabled:opacity-50"
                  >
                    Next: Choose Version
                  </button>
                </div>
              )
            ) : null}

            {/* Step 2: Version picker */}
            {selectedInstanceId && candidates.length > 0 && phase !== 'installing' && phase !== 'done' && (
              <div>
                <p className="text-xs font-medium mb-2">Available versions</p>
                {phase === 'loadingVersions' ? (
                  <div className="text-center py-4">
                    <svg className="animate-spin h-5 w-5 mx-auto text-[rgb(var(--muted))]" xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24">
                      <circle className="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" strokeWidth="4" />
                      <path className="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z" />
                    </svg>
                    <p className="text-xs text-[rgb(var(--muted))] mt-2">Loading versions…</p>
                  </div>
                ) : (
                  <ul className="space-y-2 max-h-48 overflow-y-auto">
                    {candidates.map((cand, idx) => (
                      <li
                        key={idx}
                        className={`rounded-lg border px-3 py-2 text-sm cursor-pointer transition-colors ${
                          selectedCandidate?.filename === cand.filename
                            ? 'border-brand-500 bg-brand-50 dark:bg-brand-900/20'
                            : 'border-gray-200 dark:border-gray-700 hover:bg-gray-50 dark:hover:bg-gray-800'
                        }`}
                        onClick={() => setSelectedCandidate(cand)}
                      >
                        <div className="flex items-center justify-between">
                          <span className="font-medium">{cand.version}</span>
                          {cand.is_compatible ? (
                            <span className="text-xs text-green-600 dark:text-green-400">✓ compatible</span>
                          ) : (
                            <span className="text-xs text-[rgb(var(--muted))]">may not match your instance</span>
                          )}
                        </div>
                        <p className="text-xs text-[rgb(var(--muted))] mt-0.5 truncate">{cand.filename}</p>
                        <p className="text-xs text-[rgb(var(--muted))] mt-0.5">
                          {[cand.mc_version, cand.loader].filter(Boolean).join(' · ')}
                          {cand.release_date ? ` · ${cand.release_date}` : ''}
                        </p>
                      </li>
                    ))}
                  </ul>
                )}
                {selectedCandidate && (
                  <button
                    onClick={handleConfirmInstall}
                    className="mt-3 rounded-lg bg-brand-600 px-4 py-2 text-sm font-medium text-white hover:bg-brand-700"
                  >
                    Install {selectedCandidate.filename}
                  </button>
                )}
              </div>
            )}

            {/* Empty versions */}
            {selectedInstanceId && candidates.length === 0 && phase === 'pickingVersion' && (
              <p className="text-sm text-[rgb(var(--muted))]">No compatible versions found.</p>
            )}

            {/* Step 3: Installing */}
            {phase === 'installing' && (
              <div className="text-center py-4">
                <svg className="animate-spin h-5 w-5 mx-auto text-[rgb(var(--muted))]" xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24">
                  <circle className="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" strokeWidth="4" />
                  <path className="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z" />
                </svg>
                <p className="text-xs text-[rgb(var(--muted))] mt-2">Downloading &amp; verifying…</p>
              </div>
            )}

            {/* Step 3: Done */}
            {phase === 'done' && installMsg && (
              <p className="text-sm text-green-600 dark:text-green-400">{installMsg}</p>
            )}
          </section>
        )}
      </section>

      {curatorNotes && (
        <section className="rounded-xl border border-gray-200 dark:border-gray-700 surface p-4">
          <h3 className="font-semibold text-sm mb-2">Curator Notes</h3>
          <p className="text-sm whitespace-pre-wrap text-[rgb(var(--muted))]">{curatorNotes}</p>
        </section>
      )}

      <section className="rounded-xl border border-gray-200 dark:border-gray-700 surface p-4">
        <h3 className="font-semibold text-sm mb-3">Compatible Versions</h3>
        {compatibleVersions.length === 0 ? (
          <p className="text-sm text-[rgb(var(--muted))]">No compatible version information available.</p>
        ) : (
          <ul className="space-y-1.5 text-sm">
            {compatibleVersions.map((entry, index) => (
              <li
                key={index}
                className="rounded-md border border-gray-200 dark:border-gray-700 px-3 py-1.5 break-words"
              >
                {renderVersionEntry(entry)}
              </li>
            ))}
          </ul>
        )}
      </section>

      <section className="rounded-xl border border-gray-200 dark:border-gray-700 surface p-4">
        <h3 className="font-semibold text-sm mb-3">Reviews</h3>
        {item.allow_comments ? (
          <p className="text-sm text-[rgb(var(--muted))]">
            Community reviews will appear here.
          </p>
        ) : (
          <p className="text-sm text-[rgb(var(--muted))]">
            Reviews are disabled for this mod.
          </p>
        )}
      </section>
    </div>
  );
}

function BackButton({ onBack }: { onBack: () => void }) {
  return (
    <button
      onClick={onBack}
      className="rounded-lg border border-gray-300 dark:border-gray-600 px-3 py-1.5 text-sm font-medium hover:bg-gray-100 dark:hover:bg-gray-800"
    >
      ← Back
    </button>
  );
}
