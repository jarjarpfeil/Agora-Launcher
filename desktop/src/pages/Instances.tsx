import { useEffect, useState } from 'react';
import { listen } from '@tauri-apps/api/event';
import {
  checkInstanceCrash,
  checkInstanceUpdates,
  createInstance,
  createSnapshot,
  deleteInstance,
  getSetting,
  listInstances,
  listLoaderVersions,
  listManifestLoaders,
  listManifestMcVersions,
  restoreSnapshot,
  formatError,
  type CreateInstanceRequest,
  type InstanceRow,
  type LoaderVersionSummary,
  type UpdateInfo,
} from '../lib/tauri';
import {
  resolveInstallPlan,
  applyInstallPlan,
} from '../lib/installFlow';
import { type ProcessState } from '../lib/useProcessController';
import { CrashInvestigator } from '../components/CrashInvestigator';
import {
  Dialog,
  DialogContent,
  DialogTitle,
  DialogDescription,
} from '@/components/ui/dialog';

export function Instances({
  onEditInstance,
  processState,
  processLogs,
  onStartLaunch,
  onKillProcess,
}: {
  onEditInstance: (id: string) => void;
  processState: ProcessState;
  processLogs: import('../lib/useProcessController').LogLine[];
  onStartLaunch: (instanceId: string, directLaunch: boolean) => Promise<void>;
  onKillProcess: () => Promise<void>;
}) {
  const [instances, setInstances] = useState<InstanceRow[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [showCreate, setShowCreate] = useState(false);
  const [crashInvestigation, setCrashInvestigation] = useState<{
    instanceId: string;
    crashFilename: string | null;
    manualLogText: string | null;
  } | null>(null);

  // Load direct launch mode once
  const [directLaunch, setDirectLaunch] = useState(false);

  const refresh = async () => {
    setLoading(true);
    setError(null);
    try {
      setInstances(await listInstances());
    } catch (e) {
      setError(formatError(e));
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    refresh();
  }, []);

  // Reactive crash detection when the tab becomes visible.
  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const lastLaunch = instances.find((i) => i.last_launched_at);
        if (!lastLaunch) return;
        const report = await checkInstanceCrash(lastLaunch.instance_id);
        if (!cancelled && report) {
          setCrashInvestigation({
            instanceId: lastLaunch.instance_id,
            crashFilename: report.filename,
            manualLogText: null,
          });
        }
      } catch {
        // Silently ignore — the user can still use manual troubleshooting.
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [instances]);

  // Load launch mode setting on mount
  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const mode = await getSetting('launch_mode');
        if (!cancelled) setDirectLaunch(mode === 'direct');
      } catch {
        // Default to delegation
      }
    })();
    return () => { cancelled = true; };
  }, []);

  // State for the manual crash-log paste modal.
  const [pasteLog, setPasteLog] = useState<{ open: boolean; instanceId: string } | null>(null);

  const openCrashInvestigator = (instanceId: string) => {
    setPasteLog({ open: true, instanceId });
  };

  const submitPasteLog = (text: string) => {
    if (!pasteLog) return;
    setPasteLog(null);
    setCrashInvestigation({
      instanceId: pasteLog.instanceId,
      crashFilename: null,
      manualLogText: text || null,
    });
  };

  return (
    <div className="space-y-6">
      <section className="flex items-center justify-between">
        <div>
          <h2 className="text-2xl font-bold mb-2">My Instances</h2>
          <p className="text-muted-foreground">
            Isolated modpack profiles, custom instances, and launch history.
          </p>
        </div>
        <button
          onClick={() => setShowCreate(true)}
          className="rounded-lg bg-primary px-4 py-2 text-sm font-medium text-primary-foreground hover:bg-primary/90"
        >
          + Create Instance
        </button>
      </section>

      {error && (
        <div className="rounded-lg bg-destructive p-3 text-sm text-destructive-foreground">
          {error}
        </div>
      )}

      {loading ? (
        <div className="rounded-xl p-6 border border-dashed border-border text-center text-muted-foreground">
          Loading instances…
        </div>
      ) : instances.length === 0 ? (
        <div className="rounded-xl p-6 border border-dashed border-border text-center">
          <p className="text-muted-foreground">No instances yet.</p>
          <p className="text-sm text-muted-foreground mt-2">
            Create a custom instance to install a verified modloader and launch via the official Mojang launcher or the in-app direct launcher.
          </p>
        </div>
      ) : (
        <ul className="grid grid-cols-1 gap-4 md:grid-cols-2">
          {instances.map((instance) => {
            const isRunning = processState.instanceId === instance.instance_id && processState.phase === 'running';

            const instanceLogs = processLogs.filter((l) => l.instance_id === instance.instance_id);

            return (
              <InstanceCard
                key={instance.instance_id}
                instance={instance}
                onChanged={refresh}
                onEdit={() => onEditInstance(instance.instance_id)}
                onOpenCrashInvestigator={openCrashInvestigator}
                isRunning={isRunning}
                runningPid={isRunning ? processState.pid : null}
                launchBusy={processState.phase === 'launching' || processState.phase === 'checking-health'}
                onLaunch={() => onStartLaunch(instance.instance_id, directLaunch)}
                onKill={onKillProcess}
                controllerError={processState.phase === 'failed' ? processState.error : null}
                onDismissError={() => {
                  onStartLaunch(instance.instance_id, directLaunch);
                }}
                logs={instanceLogs}
              />
            );
          })}
        </ul>
      )}

      {instances.length > 0 && (
        <UpdatesSection instances={instances} />
      )}

      {showCreate && (
        <CreateInstanceDialog
          onClose={() => setShowCreate(false)}
          onCreated={() => {
            setShowCreate(false);
            refresh();
          }}
        />
      )}

      {crashInvestigation && (
        <CrashInvestigator
          instanceId={crashInvestigation.instanceId}
          crashFilename={crashInvestigation.crashFilename}
          manualLogText={crashInvestigation.manualLogText}
          onClose={() => setCrashInvestigation(null)}
          onLaunch={() => onStartLaunch(crashInvestigation.instanceId, directLaunch)}
        />
      )}

      {pasteLog && (
        <PasteLogModal
          onClose={() => setPasteLog(null)}
          onSubmit={(text) => submitPasteLog(text)}
        />
      )}
    </div>
  );
}

function InstanceCard({
  instance,
  onChanged,
  onEdit,
  onOpenCrashInvestigator,
  isRunning,
  runningPid,
  launchBusy,
  onLaunch,
  onKill,
  controllerError,
  onDismissError,
  logs,
}: {
  instance: InstanceRow;
  onChanged: () => void;
  onEdit: () => void;
  onOpenCrashInvestigator: (id: string) => void;
  isRunning: boolean;
  runningPid: number | null;
  launchBusy: boolean;
  onLaunch: () => void;
  onKill: () => void;
  controllerError: string | null;
  onDismissError: () => void;
  logs?: import('../lib/useProcessController').LogLine[];
}) {
  const [error, setError] = useState<string | null>(null);

  const displayError = error ?? controllerError;

  const remove = async () => {
    if (!confirm(`Delete instance "${instance.name}"? This moves the folder to trash.`)) return;
    setError(null);
    try {
      await deleteInstance(instance.instance_id);
      onChanged();
    } catch (e) {
      setError(formatError(e));
    }
  };

  return (
    <li className="rounded-xl border border-border bg-card p-4">
      <div className="flex items-start justify-between gap-3">
        <div>
          <h3 className="font-semibold">{instance.name}</h3>
          <p className="text-xs text-muted-foreground">
            {instance.loader} {instance.loader_version} · MC {instance.minecraft_version}
          </p>
          <p className="text-xs text-muted-foreground mt-1">
            {isRunning ? (
              <span className="text-green-600 dark:text-green-400">● Running (PID {runningPid})</span>
            ) : instance.last_launched_at ? (
              `Last launched ${instance.last_launched_at}`
            ) : (
              'Never launched'
            )}
          </p>
        </div>
        <span className="text-xs uppercase tracking-wide text-muted-foreground">
          {instance.is_locked ? 'Locked' : 'Unlocked'}
        </span>
      </div>

      {displayError && (
        <div className="mt-2 flex items-center gap-2">
          <p className="text-xs text-destructive flex-1">{displayError}</p>
          {controllerError && (
            <button
              onClick={onDismissError}
              className="text-xs text-muted-foreground hover:underline"
            >
              Dismiss
            </button>
          )}
        </div>
      )}

      <div className="mt-4 flex flex-wrap gap-2">
        {isRunning ? (
          <button
            onClick={onKill}
            className="rounded-lg bg-destructive px-3 py-1.5 text-sm font-medium text-destructive-foreground hover:bg-destructive/90"
          >
            Kill
          </button>
        ) : (
          <button
            onClick={onLaunch}
            disabled={launchBusy}
            className="rounded-lg bg-primary px-3 py-1.5 text-sm font-medium text-primary-foreground hover:bg-primary/90 disabled:opacity-50"
          >
            {launchBusy ? 'Starting…' : 'Launch'}
          </button>
        )}
        <button
          onClick={onEdit}
          disabled={launchBusy}
          className="rounded-lg border border-border px-3 py-1.5 text-sm font-medium hover:bg-accent disabled:opacity-50"
        >
          Edit
        </button>
        <button
          onClick={() => onOpenCrashInvestigator(instance.instance_id)}
          disabled={launchBusy}
          className="rounded-lg border border-border px-3 py-1.5 text-sm font-medium hover:bg-accent disabled:opacity-50"
        >
          Troubleshoot
        </button>
        <button
          onClick={remove}
          disabled={launchBusy}
          className="rounded-lg border border-destructive/30 px-3 py-1.5 text-sm font-medium text-destructive hover:bg-destructive/10 disabled:opacity-50"
        >
          Delete
        </button>
      </div>

      {isRunning && logs && logs.length > 0 && (
        <div className="mt-3">
          <h4 className="text-xs font-semibold text-muted-foreground mb-1 uppercase tracking-wide">
            Console ({logs.length} lines)
          </h4>
          <pre className="max-h-32 overflow-y-auto rounded-lg bg-background border border-border p-2 text-[10px] font-mono leading-tight">
            {logs.slice(-200).map((l, i) => (
              <span key={i} className={l.stream === 'stderr' ? 'text-destructive' : ''}>
                {l.line}{'\n'}
              </span>
            ))}
          </pre>
        </div>
      )}
    </li>
  );
}

/** A section that checks for updates, batches them, and applies them safely. */

function UpdatesSection({
  instances,
}: {
  instances: InstanceRow[];
}) {
  const [updatesByInstance, setUpdatesByInstance] = useState<Record<string, UpdateInfo[]>>({});
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [checking, setChecking] = useState(false);
  const [updating, setUpdating] = useState(false);
  const [updateProgress, setUpdateProgress] = useState<string | null>(null);
  const [updateError, setUpdateError] = useState<string | null>(null);
  const [showConfirm, setShowConfirm] = useState<{
    instanceId: string;
    instanceName: string;
    updates: UpdateInfo[];
  } | null>(null);

  const checkAll = async () => {
    setChecking(true);
    setUpdateError(null);
    const results: Record<string, UpdateInfo[]> = {};
    for (const inst of instances) {
      if (inst.is_locked) continue; // skip locked instances
      try {
        const updates = await checkInstanceUpdates(inst.instance_id);
        if (updates.length > 0) results[inst.instance_id] = updates;
      } catch {
        // skip instances that fail
      }
    }
    setUpdatesByInstance(results);
    setSelected(new Set());
    setChecking(false);
  };

  const totalUpdates = Object.values(updatesByInstance).reduce((sum, u) => sum + u.length, 0);

  /** Toggle per-mod selection. */
  const toggleSelected = (key: string) => {
    setSelected((prev) => {
      const next = new Set(prev);
      if (next.has(key)) next.delete(key); else next.add(key);
      return next;
    });
  };

  const applyUpdates = async () => {
    if (!showConfirm) return;
    const { instanceId, updates } = showConfirm;
    setShowConfirm(null);
    setUpdating(true);
    setUpdateProgress(null);
    setUpdateError(null);

    // Filter to selected items; if nothing selected, update all.
    const toUpdate = selected.size > 0
      ? updates.filter((u) => selected.has(`${instanceId}:${u.mod_jar_id}`))
      : updates;
    if (toUpdate.length === 0) { setUpdating(false); return; }

    // Step 1: Snapshot before any mutation
    setUpdateProgress('Creating pre-update snapshot…');
    let snapshotId: string | undefined;
    try {
      const snap = await createSnapshot(instanceId, `batch-update-${Date.now()}`);
      snapshotId = snap.id;
    } catch (e) {
      setUpdateError(`Snapshot failed: ${formatError(e)} — updates cancelled.`);
      setUpdating(false);
      return;
    }

    // Step 2: Apply each selected update sequentially via the canonical pipeline.
    // For each update we construct an InstallIntent and resolve+execute through
    // the backend's `resolve_install_plan` + `apply_install_plan` Tauri commands.
    let failedCount = 0;
    for (let i = 0; i < toUpdate.length; i++) {
      const u = toUpdate[i];
      setUpdateProgress(`Updating ${i + 1}/${toUpdate.length}: ${u.filename}…`);
      try {
        // Full pipeline: resolve a plan, then execute it.
        // The backend install_pipeline handles staging, verification, and
        // atomic application.
        const intent: import('../lib/installFlow').InstallIntent = {
          action: {
            type: 'update',
            itemId: u.mod_jar_id,
            targetVersion: u.latest_version,
          },
          targetInstance: instanceId,
          optionalDeps: { type: 'exclude-all' },
          requestedBy: 'auto-update',
          overrides: { allowReplace: false, skipHealthScan: false, forceConflictResolution: {} },
        };
        const plan = await resolveInstallPlan(intent);
        const outcome = await applyInstallPlan(plan);
        if (outcome.type === 'failed') {
          throw new Error(outcome.error);
        }
        // Success via pipeline — outcome includes health scan results.
      } catch (e) {
        setUpdateError(`Update failed for ${u.filename}: ${formatError(e)}`);
        failedCount++;
        break; // Stop on first failure — rollback
      }
    }

    if (failedCount > 0 && snapshotId) {
      setUpdateProgress('Restoring snapshot…');
      try {
        await restoreSnapshot(instanceId, snapshotId);
        setUpdateError(`Update failed after ${toUpdate.length - failedCount + 1} of ${toUpdate.length} mods. Snapshot restored.`);
      } catch (rollbackErr) {
        setUpdateError(`Update failed AND snapshot restore also failed. Instance may be in an inconsistent state. Error: ${formatError(rollbackErr)}`);
      }
    } else if (failedCount === 0) {
      setUpdateProgress('All updates applied successfully.');
      await checkAll(); // refresh
    }
    setUpdating(false);
  };

  if (updating) {
    return (
      <div className="mt-6 rounded-xl border border-border bg-card p-4 space-y-3">
        <h3 className="font-semibold">Applying Updates</h3>
        <div className="flex items-center gap-2 text-sm">
          {updateProgress && !updateError && (
            <>
              <div className="h-4 w-4 animate-spin rounded-full border-2 border-primary border-t-transparent" />
              <span>{updateProgress}</span>
            </>
          )}
        </div>
        {updateError && (
          <div className="rounded-lg bg-destructive/10 p-3 text-sm text-destructive">{updateError}</div>
        )}
      </div>
    );
  }

  if (totalUpdates === 0 && !checking) {
    return (
      <div className="mt-6">
        <button
          onClick={checkAll}
          disabled={checking}
          className="rounded-lg border border-border px-4 py-2 text-sm font-medium hover:bg-accent disabled:opacity-50"
        >
          {checking ? 'Checking…' : 'Check for Updates'}
        </button>
      </div>
    );
  }

  const allSelected = (updates: UpdateInfo[], instId: string) =>
    updates.every((u) => selected.has(`${instId}:${u.mod_jar_id}`));

  return (
    <div className="mt-6 space-y-4">
      <div className="flex items-center justify-between">
        <h3 className="font-semibold">Updates Available ({totalUpdates})</h3>
        <button
          onClick={checkAll}
          disabled={checking}
          className="rounded-lg border border-border px-3 py-1.5 text-xs font-medium hover:bg-accent disabled:opacity-50"
        >
          {checking ? 'Checking…' : 'Refresh'}
        </button>
      </div>
      {Object.entries(updatesByInstance).map(([instId, updates]) => {
        const inst = instances.find((i) => i.instance_id === instId);
        const locked = inst?.is_locked ?? false;
        const selectedCount = updates.filter((u) => selected.has(`${instId}:${u.mod_jar_id}`)).length;
        return (
          <div key={instId} className="rounded-xl border border-border bg-card p-4 space-y-3">
            <div className="flex items-center justify-between">
              <p className="text-sm font-medium">{inst?.name ?? instId}</p>
              {locked && <span className="text-xs text-muted-foreground">🔒 Locked — updates disabled</span>}
            </div>
            <div className="space-y-1">
              {updates.map((u) => {
                const key = `${instId}:${u.mod_jar_id}`;
                return (
                  <div key={u.mod_jar_id} className="flex items-center gap-2 text-xs">
                    {!locked && (
                      <input
                        type="checkbox"
                        checked={selected.has(key)}
                        onChange={() => toggleSelected(key)}
                        className="rounded"
                      />
                    )}
                    <span className="flex-1">{u.filename}</span>
                    <span className="text-muted-foreground">{u.current_version} → <span className="text-primary">{u.latest_version}</span></span>
                  </div>
                );
              })}
            </div>
            {!locked && (
              <div className="flex gap-2">
                <button
                  onClick={() => {
                    // Select/deselect all for this instance
                    if (allSelected(updates, instId)) {
                      updates.forEach((u) => selected.delete(`${instId}:${u.mod_jar_id}`));
                      setSelected(new Set(selected));
                    } else {
                      updates.forEach((u) => selected.add(`${instId}:${u.mod_jar_id}`));
                      setSelected(new Set(selected));
                    }
                  }}
                  className="text-xs text-primary hover:underline"
                >
                  {allSelected(updates, instId) ? 'Deselect all' : 'Select all'}
                </button>
                {selectedCount > 0 && (
                  <button
                    onClick={() => setShowConfirm({ instanceId: instId, instanceName: inst?.name ?? instId, updates })}
                    className="rounded-lg bg-primary px-4 py-2 text-sm font-medium text-primary-foreground hover:bg-primary/90"
                  >
                    Update Selected ({selectedCount})
                  </button>
                )}
                {selectedCount === 0 && (
                  <button
                    onClick={() => {
                      updates.forEach((u) => selected.add(`${instId}:${u.mod_jar_id}`));
                      setSelected(new Set(selected));
                      setShowConfirm({ instanceId: instId, instanceName: inst?.name ?? instId, updates });
                    }}
                    className="rounded-lg bg-primary px-4 py-2 text-sm font-medium text-primary-foreground hover:bg-primary/90"
                  >
                    Update All ({updates.length})
                  </button>
                )}
              </div>
            )}
          </div>
        );
      })}

      {/* Update confirmation dialog */}
      {showConfirm && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/50" onClick={() => { if (!updating) setShowConfirm(null); }}>
          <div className="rounded-xl border border-border bg-card p-6 max-w-md w-full mx-4 shadow-xl" onClick={(e) => e.stopPropagation()}>
            <h3 className="font-semibold mb-2">Update {showConfirm.updates.filter((u) => selected.has(`${showConfirm.instanceId}:${u.mod_jar_id}`) || selected.size === 0).length} mods?</h3>
            <p className="text-xs text-muted-foreground mb-4">
              Instance: {showConfirm.instanceName}
            </p>
            <ul className="text-xs space-y-1 mb-4 max-h-40 overflow-y-auto">
              {showConfirm.updates
                .filter((u) => selected.has(`${showConfirm.instanceId}:${u.mod_jar_id}`) || selected.size === 0)
                .map((u) => (
                  <li key={u.mod_jar_id} className="flex justify-between">
                    <span>{u.filename}</span>
                    <span className="text-muted-foreground">{u.current_version} → <span className="text-primary">{u.latest_version}</span></span>
                  </li>
                ))}
            </ul>
            <p className="text-xs text-muted-foreground mb-4">
              A recovery snapshot will be created before updating. If any update fails, the snapshot will be restored.
            </p>
            <div className="flex justify-end gap-2">
              <button
                onClick={() => setShowConfirm(null)}
                disabled={updating}
                className="rounded-lg border border-border px-4 py-2 text-sm font-medium hover:bg-accent disabled:opacity-50"
              >
                Cancel
              </button>
              <button
                onClick={applyUpdates}
                disabled={updating}
                className="rounded-lg bg-primary px-4 py-2 text-sm font-medium text-primary-foreground hover:bg-primary/90 disabled:opacity-50"
              >
                {updating ? 'Updating…' : 'Update'}
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

function CreateInstanceDialog({
  onClose,
  onCreated,
}: {
  onClose: () => void;
  onCreated: () => void;
}) {
  const [name, setName] = useState('');
  const [mcVersion, setMcVersion] = useState('');
  const [loader, setLoader] = useState('fabric');
  const [loaderVersions, setLoaderVersions] = useState<LoaderVersionSummary[]>([]);
  const [loaderVersion, setLoaderVersion] = useState('');
  const [memoryMb, setMemoryMb] = useState(4096);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [progressMessage, setProgressMessage] = useState<string | null>(null);
  const [loaders, setLoaders] = useState<string[]>([]);
  const [mcVersions, setMcVersions] = useState<string[]>([]);

  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const versions = await listLoaderVersions(loader, mcVersion);
        if (cancelled) return;
        setLoaderVersions(versions);
        setLoaderVersion(versions[0]?.loader_version ?? '');
      } catch (e) {
        if (!cancelled) setError(formatError(e));
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [loader, mcVersion]);

  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const [l, v] = await Promise.all([listManifestLoaders(), listManifestMcVersions()]);
        if (!cancelled) {
          setLoaders(l);
          setMcVersions(v);
          if (!mcVersion && v.length > 0) {
            setMcVersion(v[0]);
          }
        }
      } catch {
        // Fetch failure: dropdowns render empty — acceptable degraded behavior.
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  // Re-filter MC versions when the loader changes.
  useEffect(() => {
    let cancelled = false;
    (async () => {
      if (!loader) return;
      try {
        const filtered = await listManifestMcVersions(loader);
        if (cancelled) return;
        if (filtered.length > 0) {
          setMcVersions(filtered);
          if (!filtered.includes(mcVersion)) {
            setMcVersion(filtered[0]);
          }
        }
      } catch {
        // Fetch failure — keep existing list (graceful)
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [loader]);

  // Progress event listener during creation
  useEffect(() => {
    if (!busy) return;
    setProgressMessage('Starting…');
    const unlisten = listen<{ instance_id: string; stage: string; message: string }>('instance:create-progress', (e) => {
      setProgressMessage(e.payload.message);
    });
    return () => { unlisten.then(fn => fn()); };
  }, [busy]);

  const submit = async () => {
    setBusy(true);
    setError(null);
    setProgressMessage(null);
    try {
      const instanceId = name
        .toLowerCase()
        .replace(/[^a-z0-9-_]+/g, '-')
        .replace(/^-+|-+$/g, '');
      if (!instanceId) throw new Error('Enter a valid instance name.');
      if (!loaderVersion) throw new Error('No pinned loader version selected.');

      const request: CreateInstanceRequest = {
        name,
        instance_id: instanceId,
        minecraft_version: mcVersion,
        loader,
        loader_version: loaderVersion,
        jvm_memory_mb: memoryMb,
      };
      await createInstance(request);
      onCreated();
    } catch (e) {
      setError(formatError(e));
      setBusy(false);
    }
  };

  return (
    <Dialog open onOpenChange={(open) => { if (!open) onClose(); }}>
      <DialogContent className="max-w-lg">
        <DialogTitle>Create Custom Instance</DialogTitle>
        <DialogDescription>
          Set up a new isolated modpack profile with a verified modloader.
        </DialogDescription>

        <div className="space-y-4">
          <label className="block">
            <span className="text-sm font-medium">Instance name</span>
            <input
              value={name}
              onChange={(e) => setName(e.target.value)}
              placeholder="Optimized Survival"
              className="mt-1 w-full rounded-lg border border-input bg-background px-3 py-2 text-sm"
            />
          </label>

          <div className="grid grid-cols-2 gap-4">
            <label className="block">
              <span className="text-sm font-medium">Minecraft version</span>
              <select
                value={mcVersion}
                onChange={(e) => setMcVersion(e.target.value)}
                className="mt-1 w-full rounded-lg border border-input bg-background px-3 py-2 text-sm"
              >
                {mcVersions.map((v) => (
                  <option key={v} value={v}>
                    {v}
                  </option>
                ))}
              </select>
            </label>

            <label className="block">
              <span className="text-sm font-medium">Loader</span>
              <select
                value={loader}
                onChange={(e) => setLoader(e.target.value)}
                className="mt-1 w-full rounded-lg border border-input bg-background px-3 py-2 text-sm"
              >
                {loaders.map((l) => (
                  <option key={l} value={l}>
                    {l}
                  </option>
                ))}
              </select>
            </label>
          </div>

          <label className="block">
            <span className="text-sm font-medium">Loader version</span>
            <select
              value={loaderVersion}
              onChange={(e) => setLoaderVersion(e.target.value)}
              className="mt-1 w-full rounded-lg border border-input bg-background px-3 py-2 text-sm"
            >
              {loaderVersions.length === 0 && <option value="">No pinned versions</option>}
              {loaderVersions.map((v) => (
                <option key={v.loader_version} value={v.loader_version}>
                  {v.loader_version} ({v.file_type})
                </option>
              ))}
            </select>
          </label>

          <label className="block">
            <span className="text-sm font-medium">JVM memory: {memoryMb} MB</span>
            <input
              type="range"
              min={1024}
              max={16384}
              step={512}
              value={memoryMb}
              onChange={(e) => setMemoryMb(Number(e.target.value))}
              className="mt-1 w-full accent-brand-600"
            />
          </label>
        </div>

        {progressMessage && (
          <p className="mt-4 text-sm text-muted-foreground">{progressMessage}</p>
        )}

        {error && (
          <p className="mt-4 text-sm text-destructive">{error}</p>
        )}

        <div className="mt-6 flex justify-end gap-2">
          <button
            onClick={onClose}
            disabled={busy}
            className="rounded-lg border border-input bg-background px-4 py-2 text-sm font-medium hover:bg-accent"
          >
            Cancel
          </button>
          <button
            onClick={submit}
            disabled={busy}
            className="rounded-lg bg-primary px-4 py-2 text-sm font-medium text-primary-foreground hover:bg-primary/90 disabled:opacity-50"
          >
            {busy ? 'Creating…' : 'Create'}
          </button>
        </div>
      </DialogContent>
    </Dialog>
  );
}

function PasteLogModal({
  onClose,
  onSubmit,
}: {
  onClose: () => void;
  onSubmit: (text: string) => void;
}) {
  const [text, setText] = useState('');

  return (
    <Dialog open onOpenChange={(open) => { if (!open) onClose(); }}>
      <DialogContent className="max-w-lg">
        <DialogTitle>Paste Crash Log</DialogTitle>
        <DialogDescription>
          Paste your crash log or latest.log contents for automated investigation.
        </DialogDescription>
        <textarea
          value={text}
          onChange={(e) => setText(e.target.value)}
          placeholder="Paste your crash log or latest.log contents here…"
          className="w-full h-48 rounded-lg border border-gray-300 dark:border-gray-600 bg-transparent px-3 py-2 text-sm font-mono resize-y"
        />
        <div className="mt-6 flex justify-end gap-2">
          <button
            onClick={onClose}
            className="rounded-lg border border-gray-300 dark:border-gray-600 px-4 py-2 text-sm font-medium hover:bg-gray-100 dark:hover:bg-gray-800"
          >
            Cancel
          </button>
          <button
            onClick={() => onSubmit(text)}
            className="rounded-lg bg-brand-600 px-4 py-2 text-sm font-medium text-white hover:bg-brand-700"
          >
            Investigate
          </button>
        </div>
      </DialogContent>
    </Dialog>
  );
}
