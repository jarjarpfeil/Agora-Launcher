import { useCallback, useEffect, useState } from 'react';
import { useRegistryState } from '../lib/useRegistryState';
import {
  checkInstanceCrash,
  checkInstanceUpdates,
  getSetting,
  listInstances,
  listSnapshots,
  setSetting,
  checkRegistryUpdate,
  type InstanceRow,
  type UpdateInfo,
} from '../lib/tauri';
import { useDestination } from '../lib/useDestination';

// ---------------------------------------------------------------------------
// D1: Action-oriented Home
// 4-zone layout: Alerts → Hero → Maintenance → Discovery
// ---------------------------------------------------------------------------

export function Home() {
  const { state: regState, hasCachedDb } = useRegistryState();
  const { navigateToTab, navigateToInstanceDetail } = useDestination();

  const [instances, setInstances] = useState<InstanceRow[]>([]);
  const [instancesLoading, setInstancesLoading] = useState(true);
  const [lastCrash, setLastCrash] = useState<{ instanceId: string; name: string; filename?: string } | null>(null);
  const [updatesByInstance, setUpdatesByInstance] = useState<Record<string, UpdateInfo[]>>({});
  const [snapshots, setSnapshots] = useState<{ instanceName: string; id: string; label: string }[]>([]);

  // Load instances on mount.
  const loadData = useCallback(async () => {
    setInstancesLoading(true);
    try {
      const all = await listInstances();
      setInstances(all);

      // Check for crash on the most recently launched instance.
      const launched = all.filter((i) => i.last_launched_at).sort(
        (a, b) => new Date(b.last_launched_at!).getTime() - new Date(a.last_launched_at!).getTime(),
      );
      if (launched.length > 0) {
        const latest = launched[0];
        try {
          const crash = await checkInstanceCrash(latest.instance_id);
          if (crash) {
            setLastCrash({ instanceId: latest.instance_id, name: latest.name, filename: crash.filename ?? undefined });
          } else {
            setLastCrash(null);
          }
        } catch { setLastCrash(null); }
      } else {
        setLastCrash(null);
      }

      // Check for updates on launched instances.
      const updates: Record<string, UpdateInfo[]> = {};
      for (const inst of all) {
        if (inst.is_locked) continue;
        try {
          const u = await checkInstanceUpdates(inst.instance_id);
          if (u.length > 0) updates[inst.instance_id] = u;
        } catch { /* skip */ }
      }
      setUpdatesByInstance(updates);

      // Check for snapshots (LKG candidates) on instances with launches.
      const snapResults: { instanceName: string; id: string; label: string }[] = [];
      for (const inst of launched.slice(0, 3)) {
        try {
          const snapList = await listSnapshots(inst.instance_id);
          for (const s of snapList) {
            snapResults.push({ instanceName: inst.name, id: s.id, label: s.label ?? '' });
          }
        } catch { /* skip */ }
      }
      setSnapshots(snapResults);
    } catch { /* ignore */ }
    setInstancesLoading(false);
  }, []);

  useEffect(() => {
    loadData();
  }, [loadData]);

  // Track last home visit for change detection.
  useEffect(() => {
    getSetting('last_home_visit').catch(() => {});
    return () => {
      setSetting('last_home_visit', new Date().toISOString()).catch(() => {});
    };
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Group cards by zone.
  const sortedByLaunched = [...instances].sort(
    (a, b) => new Date(b.last_launched_at ?? 0).getTime() - new Date(a.last_launched_at ?? 0).getTime(),
  );
  const lastLaunched = sortedByLaunched[0] ?? null;
  const totalUpdates = Object.values(updatesByInstance).reduce((s, u) => s + u.length, 0);
  const heroInstance = lastLaunched ?? sortedByLaunched[0] ?? null;

  return (
    <div className="space-y-6">
      {/* Header */}
      <section>
        <h2 className="text-2xl font-bold mb-2">Home</h2>
        <p className="text-muted-foreground">Your modding dashboard.</p>
      </section>

      {/* Zone A: Alerts — compact warnings */}
      {lastCrash && (
        <CrashAlert
          instanceName={lastCrash.name}
          crashFilename={lastCrash.filename}
          onRestore={() => navigateToInstanceDetail(lastCrash.instanceId)}
        />
      )}

      {regState === 'missing' && (
        <RegistryAlert
          hasCachedDb={hasCachedDb}
        />
      )}

      {/* Zone B: Hero — Continue Playing */}
      <ContinuePlayingCard
        instance={heroInstance}
        loading={instancesLoading}
        onLaunch={() => {
          if (heroInstance) {
            navigateToInstanceDetail(heroInstance.instance_id);
          }
        }}
        onBrowsePacks={() => navigateToTab('browse')}
      />

      {/* Zone C: Maintenance — only when triggered */}
      {totalUpdates > 0 && (
        <UpdatesCard
          totalUpdates={totalUpdates}
          onReview={() => navigateToTab('instances')}
        />
      )}

      {snapshots.length > 0 && (
        <SnapshotsCard
          snapshots={snapshots}
          onRestore={(id) => navigateToInstanceDetail(id)}
        />
      )}

      {/* Zone D: Discovery — always present */}
      <RecommendationsCard
        hasInstances={instances.length > 0}
        hasCachedDb={hasCachedDb}
        loading={instancesLoading}
        activeInstance={lastLaunched}
        onBrowseMore={() => navigateToTab('browse')}
      />
    </div>
  );
}

// ---------------------------------------------------------------------------
// Card components
// ---------------------------------------------------------------------------

function CrashAlert({ instanceName, crashFilename, onRestore }: {
  instanceName: string;
  crashFilename?: string;
  onRestore: () => void;
}) {
  return (
    <div className="rounded-lg border border-destructive bg-destructive/10 p-3 flex items-center justify-between gap-3">
      <div className="text-xs text-destructive flex-1">
        <span className="font-semibold">{instanceName}</span> did not exit cleanly.
        {crashFilename && <span className="text-muted-foreground ml-1">({crashFilename})</span>}
      </div>
      <button onClick={onRestore} className="rounded-lg bg-destructive px-3 py-1.5 text-xs font-medium text-destructive-foreground hover:bg-destructive/90">
        View & restore
      </button>
    </div>
  );
}

function RegistryAlert({ hasCachedDb }: {
  hasCachedDb: boolean;
}) {
  const [downloading, setDownloading] = useState(false);

  const handleDownload = async () => {
    setDownloading(true);
    try {
      await checkRegistryUpdate(true);
    } catch {
      // error handled by the registry status view
    } finally {
      setDownloading(false);
    }
  };

  return (
    <div className="rounded-lg border border-amber-500 bg-amber-50 dark:bg-amber-900/20 p-3 flex items-center justify-between gap-3">
      <p className="text-xs text-amber-700 dark:text-amber-300">
        {hasCachedDb
          ? 'Using cached registry — updates, recommendations, and governance are offline.'
          : 'Registry not downloaded yet. Download it to enable updates, recommendations, and governance.'}
      </p>
      <button onClick={handleDownload} disabled={downloading} className="rounded-lg bg-amber-600 px-3 py-1.5 text-xs font-medium text-white hover:bg-amber-700 disabled:opacity-50">
        {downloading ? 'Downloading…' : 'Download registry'}
      </button>
    </div>
  );
}

function ContinuePlayingCard({ instance, loading, onLaunch, onBrowsePacks }: {
  instance: InstanceRow | null;
  loading: boolean;
  onLaunch: () => void;
  onBrowsePacks: () => void;
}) {
  if (loading) {
    return (
      <div className="rounded-xl border border-border bg-card p-6 space-y-2">
        <div className="h-5 w-32 bg-muted animate-pulse rounded" />
        <div className="h-4 w-48 bg-muted animate-pulse rounded" />
      </div>
    );
  }

  if (!instance) {
    return (
      <div className="rounded-xl border border-border bg-card p-6">
        <h3 className="text-lg font-semibold mb-2">Welcome to Agora</h3>
        <p className="text-sm text-muted-foreground mb-4">
          No instances yet. Create one from a mod pack to start playing.
        </p>
        <button onClick={onBrowsePacks} className="rounded-lg bg-primary px-5 py-2 text-sm font-medium text-primary-foreground hover:bg-primary/90">
          Browse mod packs
        </button>
      </div>
    );
  }

  const timeAgo = instance.last_launched_at
    ? timeSince(new Date(instance.last_launched_at))
    : 'Not launched yet';

  return (
    <div className="rounded-xl border border-border bg-card p-6">
      <h3 className="text-lg font-semibold mb-1">{instance.name}</h3>
      <p className="text-xs text-muted-foreground mb-1">
        {instance.loader} {instance.loader_version} · MC {instance.minecraft_version}
      </p>
      <p className="text-xs text-muted-foreground mb-4">{timeAgo}</p>
      <div className="flex gap-2">
        <button onClick={onLaunch} className="rounded-lg bg-primary px-5 py-2 text-sm font-medium text-primary-foreground hover:bg-primary/90">
          Continue Playing
        </button>
      </div>
    </div>
  );
}

function UpdatesCard({ totalUpdates, onReview }: {
  totalUpdates: number;
  onReview: () => void;
}) {
  return (
    <div className="rounded-xl border border-border bg-card p-4 flex items-center justify-between">
      <div>
        <h4 className="font-semibold text-sm">Updates Available</h4>
        <p className="text-xs text-muted-foreground">{totalUpdates} mods can be updated</p>
      </div>
      <button onClick={onReview} className="rounded-lg bg-primary px-4 py-2 text-sm font-medium text-primary-foreground hover:bg-primary/90">
        Review
      </button>
    </div>
  );
}

function SnapshotsCard({
  snapshots,
  onRestore,
}: {
  snapshots: { instanceName: string; id: string; label: string }[];
  onRestore: (id: string) => void;
}) {
  return (
    <div className="rounded-xl border border-border bg-card p-4 space-y-2">
      <h4 className="font-semibold text-sm">Snapshots</h4>
      <div className="space-y-1">
        {snapshots.slice(0, 3).map((s) => (
          <div key={s.id} className="flex items-center justify-between text-xs">
            <span>{s.instanceName}: {s.label}</span>
            <button onClick={() => onRestore(s.id)} className="text-primary hover:underline text-xs">Restore</button>
          </div>
        ))}
      </div>
    </div>
  );
}

function RecommendationsCard({ hasInstances, hasCachedDb, loading, activeInstance, onBrowseMore }: {
  hasInstances: boolean;
  hasCachedDb: boolean;
  loading: boolean;
  activeInstance: InstanceRow | null;
  onBrowseMore: () => void;
}) {
  if (loading) return null;

  if (!hasInstances) {
    return (
      <div className="rounded-xl border border-dashed border-border p-6 text-center">
        <p className="text-muted-foreground">
          Once you have an instance, we&apos;ll show mods that work with it.
        </p>
        <button onClick={onBrowseMore} className="mt-3 rounded-lg border border-border px-4 py-2 text-sm font-medium hover:bg-accent">
          Browse all mods
        </button>
      </div>
    );
  }

  if (!hasCachedDb) {
    return (
      <div className="rounded-xl border border-dashed border-border p-6 text-center">
        <p className="text-muted-foreground">
          Download the registry to see compatible recommendations.
        </p>
      </div>
    );
  }

  const label = activeInstance
    ? `Compatible recommendations for ${activeInstance.name}`
    : 'Curated mods and packs';

  return (
    <div className="rounded-xl border border-dashed border-border p-6 text-center">
      <p className="text-muted-foreground">{label}</p>
      <button onClick={onBrowseMore} className="mt-3 rounded-lg bg-primary px-5 py-2 text-sm font-medium text-primary-foreground hover:bg-primary/90">
        Browse more
      </button>
    </div>
  );
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function timeSince(date: Date): string {
  const now = new Date();
  const diffMs = now.getTime() - date.getTime();
  const mins = Math.floor(diffMs / 60000);
  if (mins < 1) return 'Just now';
  if (mins < 60) return `${mins}m ago`;
  const hours = Math.floor(mins / 60);
  if (hours < 24) return `${hours}h ago`;
  const days = Math.floor(hours / 24);
  if (days < 7) return `${days}d ago`;
  return date.toLocaleDateString();
}
